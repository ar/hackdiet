use crate::{
    stats::{self, Day},
    Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::NaiveDate;
use plotters::prelude::*;
use std::{env, io::Write, path::Path};

pub fn supported() -> bool {
    // Multiplexers need their own passthrough handling; don't emit raw images there.
    if env::var_os("TMUX").is_some() || env::var_os("STY").is_some() {
        return false;
    }
    env::var("TERM_PROGRAM").is_ok_and(|s| s == "ghostty" || s == "kitty")
        || env::var("TERM").is_ok_and(|s| s == "xterm-ghostty" || s == "xterm-kitty")
}

pub fn render(
    path: &Path,
    days: &[Day],
    start: NaiveDate,
    end: NaiveDate,
    height: Option<f64>,
) -> Result<()> {
    let bg = RGBColor(250, 250, 247);
    let red = RGBColor(197, 55, 58);
    let green = RGBColor(43, 132, 109);
    let ink = RGBColor(42, 49, 59);
    let root = BitMapBackend::new(path, (1200, 600)).into_drawing_area();
    root.fill(&bg)?;
    // Give the summary its own space so it never obscures the weight curves.
    let (header, plot) = root.split_vertically(142);
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
    let span = (end - start).num_days().max(1) as f64;
    let weight_range = (min - padding)..(max + padding);
    let height_squared = height.map(|cm| (cm / 100.0).powi(2));
    let mut chart = ChartBuilder::on(&plot)
        .margin(24)
        .margin_top(8)
        .x_label_area_size(50)
        .y_label_area_size(75)
        .right_y_label_area_size(if height.is_some() { 75 } else { 0 })
        .build_cartesian_2d(-0.15..span + 0.15, weight_range.clone())?
        .set_secondary_coord(
            -0.15..span + 0.15,
            (weight_range.start / height_squared.unwrap_or(1.0))
                ..(weight_range.end / height_squared.unwrap_or(1.0)),
        );
    let label = |x: &f64| {
        if *x < 0.0 || *x > span || (x - x.round()).abs() > 0.01 {
            return String::new();
        }
        let date = start + chrono::Duration::days(x.round() as i64);
        if span > 365.0 {
            date.format("%b %Y").to_string()
        } else {
            date.format("%d %b").to_string()
        }
    };
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
    Ok(())
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

pub fn display(png: &[u8]) -> Result<()> {
    let (width, height) = terminal_size::terminal_size()
        .map(|(w, h)| (w.0, h.0))
        .unwrap_or((100, 30));
    let columns = width.saturating_sub(1).clamp(1, 120);
    let rows = (columns / 4)
        .max(1)
        .min(height.saturating_sub(6).clamp(1, 24));
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
