use crate::{
    log::{Log, Metric, Section},
    stats::{self, Day, ExerciseDay},
    Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{Datelike, NaiveDate, Weekday};
use plotters::{element::DashedPathElement, prelude::*};
use std::{collections::BTreeMap, env, io::Write, path::Path};

pub fn supported() -> bool {
    // Multiplexers need their own passthrough handling; don't emit raw images there.
    if env::var_os("TMUX").is_some() || env::var_os("STY").is_some() {
        return false;
    }
    env::var("TERM_PROGRAM").is_ok_and(|s| s == "ghostty" || s == "kitty")
        || env::var("TERM").is_ok_and(|s| s == "xterm-ghostty" || s == "xterm-kitty")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Panel {
    Weight,
    Exercise,
    Events,
    Metrics(i32, i32),
}

pub struct Timeline<'a> {
    panels: Vec<Panel>,
    start: NaiveDate,
    end: NaiveDate,
    pub groups: BTreeMap<(i32, i32), Vec<(usize, &'a Metric)>>,
    pub events: Vec<(NaiveDate, &'a str)>,
}

impl<'a> Timeline<'a> {
    pub fn new(log: &'a Log, start: NaiveDate, end: NaiveDate) -> Self {
        let mut groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
        let mut style_indices = BTreeMap::<_, usize>::new();
        let mut panels = Vec::new();
        for section in log.sections() {
            match section {
                Section::Weight => panels.push(Panel::Weight),
                Section::Exercise => panels.push(Panel::Exercise),
                Section::Events => panels.push(Panel::Events),
                Section::Metric(code) => {
                    let metric = &log.metrics[code];
                    let range = (metric.min, metric.max);
                    let next_style = style_indices.entry(range).or_default();
                    if metric.ratings.range(start..=end).next().is_some() {
                        let group = groups.entry(range).or_default();
                        if group.is_empty() {
                            panels.push(Panel::Metrics(metric.min, metric.max));
                        }
                        group.push((*next_style, metric));
                    }
                    *next_style += 1;
                }
                Section::Text(_) | Section::Height => {}
            }
        }
        let events = log
            .events
            .range(start..=end)
            .flat_map(|(&date, labels)| labels.iter().map(move |label| (date, label.as_str())))
            .collect();
        Self {
            panels,
            start,
            end,
            groups,
            events,
        }
    }

    fn x(&self, date: NaiveDate) -> f64 {
        (date - self.start).num_days() as f64
    }

    fn guides(
        &self,
        chart: &mut ChartContext<
            '_,
            BitMapBackend<'_>,
            Cartesian2d<
                plotters::coord::types::RangedCoordf64,
                plotters::coord::types::RangedCoordf64,
            >,
        >,
        low: f64,
        high: f64,
    ) -> Result<()> {
        for date in self
            .start
            .iter_days()
            .take_while(|d| *d <= self.end)
            .filter(|d| d.weekday() == Weekday::Sun)
        {
            let x = self.x(date);
            chart.draw_series(DashedLineSeries::new(
                [(x, low), (x, high)],
                4,
                4,
                RGBColor(42, 49, 59).mix(0.3).stroke_width(1),
            ))?;
        }
        let mut previous = None;
        for &(date, _) in &self.events {
            if previous == Some(date) {
                continue;
            }
            previous = Some(date);
            let x = self.x(date);
            chart.draw_series(DashedLineSeries::new(
                [(x, low), (x, high)],
                8,
                3,
                EVENT_COLOR.stroke_width(2),
            ))?;
        }
        Ok(())
    }
}

const EVENT_COLOR: RGBColor = RGBColor(142, 93, 37);

pub fn render(
    path: &Path,
    days: &[Day],
    exercise: &[ExerciseDay],
    timeline: &Timeline,
    height: Option<f64>,
) -> Result<u32> {
    let (start, end) = (timeline.start, timeline.end);
    let bg = RGBColor(250, 250, 247);
    let red = RGBColor(197, 55, 58);
    let green = RGBColor(43, 132, 109);
    let ink = RGBColor(42, 49, 59);
    let sunday = ink.mix(0.3).stroke_width(1);
    let combined = !days.is_empty() && !exercise.is_empty();
    let weight_height = if days.is_empty() { 0 } else { 600 };
    let exercise_height = if combined {
        240
    } else if exercise.is_empty() {
        0
    } else {
        600
    };
    let event_height = if timeline.events.is_empty() {
        0
    } else {
        50 + timeline.events.len() as u32 * 28
    };
    let metric_height = |count: usize| 240 + 30 * count as u32;
    let panel_height = |panel: &Panel| match *panel {
        Panel::Weight => weight_height,
        Panel::Exercise => exercise_height,
        Panel::Events => event_height,
        Panel::Metrics(min, max) => metric_height(timeline.groups[&(min, max)].len()),
    };
    let image_height = timeline.panels.iter().map(panel_height).sum();
    let root = BitMapBackend::new(path, (1200, image_height)).into_drawing_area();
    root.fill(&bg)?;
    let mut remaining = root.clone();
    let mut areas = BTreeMap::new();
    for panel in &timeline.panels {
        let (area, rest) = remaining.split_vertically(panel_height(panel));
        areas.insert(*panel, area);
        remaining = rest;
    }
    let span = (end - start).num_days().max(1) as f64;
    let x_range = -0.4..span + 0.4;
    let label = |x: &f64| {
        if *x < 0.0 || *x > (end - start).num_days() as f64 || (x - x.round()).abs() > 0.01 {
            return String::new();
        }
        let date = start + chrono::Duration::days(x.round() as i64);
        date.format(if span > 365.0 { "%b %Y" } else { "%d %b" })
            .to_string()
    };
    let right_axis = if height.is_some() { 75 } else { 0 };
    if !timeline.events.is_empty() {
        let events = &areas[&Panel::Events];
        events.draw(&Text::new(
            format!("Events  |  {}", stats::period_label(start, end)),
            (24, 12),
            ("sans-serif", 23).into_font().color(&ink),
        ))?;
        let rows = timeline.events.len() as i32;
        let (_, strip) = events.split_vertically(44);
        let chart = ChartBuilder::on(&strip)
            .margin_left(24)
            .margin_right(24)
            .y_label_area_size(75)
            .right_y_label_area_size(right_axis)
            .build_cartesian_2d(x_range.clone(), 0..rows)?;
        for (i, &(date, name)) in timeline.events.iter().enumerate() {
            let x = chart
                .plotting_area()
                .map_coordinate(&(timeline.x(date), 0))
                .0;
            let y = 14 + i as i32 * 28;
            let style = ("sans-serif", 19).into_font().color(&EVENT_COLOR);
            let caption = format!("{} · {name}", date.format("%d %b"));
            let width = strip.estimate_text_size(&caption, &style)?.0 as i32;
            let left = if x + 8 + width < 1190 {
                x + 8
            } else {
                (x - width - 8).max(4)
            };
            strip.draw(&Circle::new((x, y), 4, EVENT_COLOR.filled()))?;
            strip.draw(&Text::new(caption, (left, y - 9), style))?;
        }
    }
    for (&(min, max), group) in &timeline.groups {
        let area = &areas[&Panel::Metrics(min, max)];
        let (heading, panel) = area.split_vertically(30 * group.len() as u32 + 12);
        let mut chart = ChartBuilder::on(&panel)
            .margin(24)
            .margin_top(8)
            .x_label_area_size(50)
            .y_label_area_size(75)
            .right_y_label_area_size(right_axis)
            .build_cartesian_2d(x_range.clone(), f64::from(min)..f64::from(max))?;
        chart
            .configure_mesh()
            .x_labels(7)
            .x_label_formatter(&label)
            .y_labels(6)
            .y_desc("rating")
            .label_style(("sans-serif", 20))
            .disable_x_mesh()
            .max_light_lines(0)
            .draw()?;
        timeline.guides(&mut chart, min.into(), max.into())?;
        let colors = [
            RGBColor(30, 109, 155),
            RGBColor(155, 71, 132),
            RGBColor(34, 128, 96),
            RGBColor(178, 94, 45),
        ];
        for (i, &(style_index, metric)) in group.iter().enumerate() {
            let color = colors
                .get(style_index)
                .map(Color::to_rgba)
                .unwrap_or_else(|| Palette99::pick(style_index).to_rgba());
            let y = 19 + i as i32 * 30;
            let (latest, (value, _)) = metric.ratings.range(start..=end).next_back().unwrap();
            let caption = format!(
                "{}  ·  {min}–{max}   |   {value} on {}",
                metric.name,
                latest.format("%d %b %Y")
            );
            heading.draw(&Text::new(
                caption,
                (52, y - 10),
                ("sans-serif", 21).into_font().color(&ink),
            ))?;
            let points = metric
                .ratings
                .range(start..=end)
                .map(|(&date, (value, _))| (timeline.x(date), f64::from(*value)));
            for (a, b) in points
                .clone()
                .zip(points.clone().skip(1))
                .filter(|(a, b)| b.0 - a.0 == 1.0)
            {
                chart.draw_series([PathElement::new([a, b], color.stroke_width(2))])?;
            }
            if style_index % 2 == 0 {
                heading.draw(&Circle::new((32, y), 6, color.stroke_width(2)))?;
                chart.draw_series(points.map(|p| Circle::new(p, 6, color.stroke_width(2))))?;
            } else {
                heading.draw(&TriangleMarker::new((32, y), 5, color.filled()))?;
                chart.draw_series(points.map(|p| TriangleMarker::new(p, 5, color.filled())))?;
            }
        }
    }
    if !exercise.is_empty() {
        let blue = RGBColor(45, 97, 155);
        let bar = RGBColor(157, 197, 220);
        let (heading, panel) =
            areas[&Panel::Exercise].split_vertically(if combined { 46 } else { 85 });
        heading.draw(&Text::new(
            format!("Exercise  |  {}", stats::exercise_summary(exercise)),
            (24, 10),
            ("sans-serif", 23).into_font().color(&ink),
        ))?;
        heading.draw(&PathElement::new(
            [(800, 22), (835, 22)],
            blue.stroke_width(3),
        ))?;
        heading.draw(&Text::new(
            "7-day average (kcal/day)",
            (845, 10),
            ("sans-serif", 20).into_font().color(&ink),
        ))?;
        if !combined {
            heading.draw(&Text::new(
                stats::period_label(start, end),
                (24, 47),
                ("sans-serif", 22).into_font().color(&ink),
            ))?;
        }
        let max = exercise
            .iter()
            .map(|d| d.average.max(d.kcal as f64))
            .fold(1.0, f64::max);
        let mut chart = ChartBuilder::on(&panel)
            .margin(24)
            .margin_top(8)
            .x_label_area_size(50)
            .y_label_area_size(75)
            .right_y_label_area_size(right_axis)
            .build_cartesian_2d(x_range.clone(), 0.0..max * 1.08)?;
        chart
            .configure_mesh()
            .x_labels(7)
            .x_label_formatter(&label)
            .y_labels(4)
            .label_style(("sans-serif", 20))
            .y_desc("active kcal")
            .max_light_lines(0)
            .disable_x_mesh()
            .draw()?;
        chart.draw_series(exercise.iter().enumerate().filter(|(_, d)| d.kcal > 0).map(
            |(i, d)| {
                let x = i as f64;
                Rectangle::new([(x - 0.32, 0.0), (x + 0.32, d.kcal as f64)], bar.filled())
            },
        ))?;
        timeline.guides(&mut chart, 0.0, max * 1.08)?;
        chart.draw_series(LineSeries::new(
            exercise
                .iter()
                .enumerate()
                .map(|(i, d)| (i as f64, d.average)),
            blue.stroke_width(3),
        ))?;
        if exercise.len() == 1 {
            chart.draw_series([Circle::new((0.0, exercise[0].average), 4, blue.filled())])?;
        }
    }
    if days.is_empty() {
        root.present()?;
        return Ok(image_height);
    }
    // Give the summary its own space so it never obscures the weight curves.
    let (header, plot) = areas[&Panel::Weight].split_vertically(142);
    header.draw(&Text::new(
        format!("Weight  |  {}", stats::period_label(start, end)),
        (24, 28),
        ("sans-serif", 26).into_font().color(&ink),
    ))?;
    let metric = |value: String, label: &str, x: i32, color: RGBColor| -> Result<()> {
        header.draw(&Text::new(
            value,
            (x, 79),
            ("sans-serif", 38).into_font().color(&color.mix(0.65)),
        ))?;
        header.draw(&Text::new(
            label,
            (x, 116),
            ("sans-serif", 17).into_font().color(&color.mix(0.65)),
        ))?;
        Ok(())
    };
    if let Some(labels) = stats::change_labels(days) {
        for ((value, label), x) in labels.into_iter().zip([100, 370]) {
            let color = if matches!(label, "weekly gain" | "estimated excess") {
                red
            } else {
                ink
            };
            metric(value, label, x, color)?;
        }
    } else {
        header.draw(&Text::new(
            "Two weigh-ins needed to estimate change",
            (100, 87),
            ("sans-serif", 22).into_font().color(&ink.mix(0.65)),
        ))?;
    }
    if let Some((_, latest)) = height.and_then(|h| stats::bmi(days, h)) {
        metric(format!("{latest:.1}"), "BMI", 660, ink)?;
    }
    if let Some(latest) = days.last() {
        let label = if latest.date == end {
            "latest trend".into()
        } else {
            format!("latest trend · {}", latest.date.format("%d %b"))
        };
        metric(
            format!("{:.2} kg", latest.trend),
            &label,
            if height.is_some() { 875 } else { 660 },
            ink,
        )?;
    }
    header.draw(&DashedPathElement::new(
        vec![(580, 37), (608, 37)],
        4,
        4,
        sunday,
    ))?;
    header.draw(&Text::new(
        "Sunday",
        (620, 30),
        ("sans-serif", 17).into_font().color(&ink),
    ))?;
    header.draw(&PathElement::new(
        vec![(760, 37), (788, 37)],
        red.stroke_width(3),
    ))?;
    header.draw(&Text::new(
        "Smoothed trend",
        (800, 30),
        ("sans-serif", 17).into_font().color(&ink),
    ))?;
    header.draw(&PathElement::new(
        vec![(985, 32), (990, 37), (985, 42), (980, 37), (985, 32)],
        green.stroke_width(2),
    ))?;
    header.draw(&Text::new(
        "Daily weight",
        (1005, 30),
        ("sans-serif", 17).into_font().color(&ink),
    ))?;
    let min = days
        .iter()
        .flat_map(|d| [d.trend, d.weight.unwrap_or(d.trend)])
        .fold(f64::INFINITY, f64::min);
    let max = days
        .iter()
        .flat_map(|d| [d.trend, d.weight.unwrap_or(d.trend)])
        .fold(f64::NEG_INFINITY, f64::max);
    let padding = ((max - min) * 0.18).max(0.2);
    let weight_range = (min - padding)..(max + padding);
    let height_squared = height.map(|cm| (cm / 100.0).powi(2));
    let mut chart = ChartBuilder::on(&plot)
        .margin(24)
        .margin_top(8)
        .x_label_area_size(50)
        .y_label_area_size(75)
        .right_y_label_area_size(right_axis)
        .build_cartesian_2d(x_range.clone(), weight_range.clone())?
        .set_secondary_coord(
            x_range.clone(),
            (weight_range.start / height_squared.unwrap_or(1.0))
                ..(weight_range.end / height_squared.unwrap_or(1.0)),
        );
    chart
        .configure_mesh()
        .x_labels(7)
        .y_labels(7)
        .x_label_formatter(&label)
        .y_label_formatter(&|y| format!("{y:.1}"))
        .y_desc("kg")
        .label_style(("sans-serif", 20).into_font().color(&ink))
        .axis_desc_style(("sans-serif", 20))
        .axis_style(RGBColor(160, 165, 170))
        .light_line_style(RGBColor(235, 236, 231))
        .bold_line_style(RGBColor(220, 223, 217))
        .draw()?;
    if height.is_some() {
        chart
            .configure_secondary_axes()
            .y_labels(7)
            .y_label_formatter(&|y| format!("{y:.1}"))
            .y_desc("BMI")
            .label_style(("sans-serif", 20).into_font().color(&ink))
            .axis_desc_style(("sans-serif", 20))
            .axis_style(RGBColor(160, 165, 170))
            .draw()?;
    }
    timeline.guides(&mut chart, weight_range.start, weight_range.end)?;
    let x = |d: &Day| (d.date - start).num_days() as f64;
    chart.draw_series(LineSeries::new(
        days.iter().map(|d| (x(d), d.trend)),
        red.stroke_width(3),
    ))?;
    // A dot keeps the initial trend visible even with a single entry.
    if days.len() == 1 {
        chart.draw_series(std::iter::once(Circle::new(
            (x(&days[0]), days[0].trend),
            4,
            red.filled(),
        )))?;
    }
    if span <= 92.0 {
        chart.draw_series(days.iter().filter_map(|d| {
            d.weight
                .map(|w| PathElement::new(vec![(x(d), d.trend), (x(d), w)], green.stroke_width(2)))
        }))?;
    } else {
        chart.draw_series(LineSeries::new(
            days.iter().filter_map(|d| d.weight.map(|w| (x(d), w))),
            RGBColor(170, 180, 177).stroke_width(1),
        ))?;
    }
    chart.draw_series(PointSeries::of_element(
        days.iter().filter_map(|d| d.weight.map(|w| (x(d), w))),
        5,
        green.stroke_width(2),
        &|point, size, style| {
            EmptyElement::at(point)
                + Polygon::new(
                    vec![(0, -size), (size, 0), (0, size), (-size, 0)],
                    bg.filled(),
                )
                + PathElement::new(
                    vec![(0, -size), (size, 0), (0, size), (-size, 0), (0, -size)],
                    style,
                )
        },
    ))?;
    root.present()?;
    Ok(image_height)
}

fn transmit(out: &mut impl Write, png: &[u8], columns: u16, rows: u16) -> std::io::Result<()> {
    let encoded = STANDARD.encode(png);
    let chunks: Vec<_> = encoded.as_bytes().chunks(4096).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        if i == 0 {
            write!(
                out,
                "\x1b_Ga=T,f=100,t=d,q=2,C=1,c={columns},r={rows},m={more};"
            )?;
        } else {
            write!(out, "\x1b_Gm={more};")?;
        }
        out.write_all(chunk)?;
        out.write_all(b"\x1b\\")?;
    }
    Ok(())
}

pub fn display(png: &[u8], image_height: u32) -> Result<()> {
    let (width, height) = terminal_size::terminal_size()
        .map(|(w, h)| (w.0, h.0))
        .unwrap_or((100, 30));
    // Fit both dimensions while preserving the aspect ratio on tall timelines.
    // Terminal cells are roughly twice as tall as they are wide.
    let available_rows = u64::from(height.saturating_sub(6).max(1));
    let image_height = u64::from(image_height);
    let columns = u64::from(width.saturating_sub(1).clamp(1, 120))
        .min((available_rows * 2400 / image_height).max(1)) as u16;
    let rows = (u64::from(columns) * image_height / 2400).clamp(1, available_rows) as u16;
    let mut out = std::io::stdout().lock();
    // Reserve space first, including at the bottom of the screen, then place the
    // image there and leave the shell prompt below it. No raw mode or event loop.
    for _ in 0..rows {
        writeln!(out)?;
    }
    write!(out, "\x1b[{rows}A\r")?;
    transmit(&mut out, png, columns, rows)?;
    write!(out, "\x1b[{rows}B\r")?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panels_and_shared_scale_legends_follow_markdown_order() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        writeln!(file, "| Date | Active kcal | Description |\n|---|---|---|").unwrap();
        for (code, max, visible) in [
            ("unused", 5, false),
            ("load", 10, true),
            ("z", 5, true),
            ("a", 5, true),
        ] {
            writeln!(
                file,
                "\n## Metric: {code} — {code} (0–{max})\n\n| Date | Value | Note |\n|---|---|---|"
            )
            .unwrap();
            if visible {
                writeln!(file, "| {date} | 2 | |").unwrap();
            }
            if code == "z" {
                writeln!(file, "\n| Date | Event |\n|---|---|\n| {date} | Shift |").unwrap();
            }
        }
        writeln!(file, "\n| Date | Weight (kg) | Note |\n|---|---|---|").unwrap();
        let log = Log::read(file.path()).unwrap();
        let timeline = Timeline::new(&log, date, date);
        assert_eq!(
            timeline.panels,
            [
                Panel::Exercise,
                Panel::Metrics(0, 10),
                Panel::Metrics(0, 5),
                Panel::Events,
                Panel::Weight
            ]
        );
        let names: Vec<_> = timeline.groups[&(0, 5)]
            .iter()
            .map(|(_, metric)| metric.name.as_str())
            .collect();
        assert_eq!(names, ["z", "a"]);
    }

    #[test]
    fn kitty_payload_roundtrips_in_bounded_chunks() {
        let bytes: Vec<_> = (0..20000).map(|i| (i % 251) as u8).collect();
        let mut out = Vec::new();
        transmit(&mut out, &bytes, 80, 20).unwrap();
        let s = String::from_utf8(out).unwrap();
        let mut encoded = String::new();
        let chunks: Vec<_> = s.split("\x1b\\").filter(|s| !s.is_empty()).collect();
        for (i, c) in chunks.iter().enumerate() {
            let (header, data) = c.split_once(';').unwrap();
            assert!(data.len() <= 4096);
            assert!(header.ends_with(if i + 1 < chunks.len() { "m=1" } else { "m=0" }));
            encoded.push_str(data);
        }
        assert_eq!(STANDARD.decode(encoded).unwrap(), bytes);
    }
}
