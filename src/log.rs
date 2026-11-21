use crate::{positive, Result};
use chrono::NaiveDate;
use std::{collections::BTreeMap, fs, io::Write, path::Path};

#[cfg(test)]
const HEADER: &str = "| Date | Weight (kg) | Note |";

#[derive(Clone, Debug)]
pub struct Entry {
    pub weight: f64,
    pub note: String,
}

#[derive(Debug)]
pub struct Workout {
    pub kcal: u32,
    pub note: String,
}

#[derive(Debug)]
pub struct Metric {
    pub name: String,
    pub min: i32,
    pub max: i32,
    pub ratings: BTreeMap<NaiveDate, (i32, String)>,
}

impl Metric {
    pub fn new(code: &str, name: &str, min: i32, max: i32) -> Result<Self> {
        if code.is_empty()
            || code == "define"
            || !code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err("Choose a metric code using letters, digits, hyphens or underscores (define is reserved)".into());
        }
        if name.trim().is_empty() || name.chars().any(char::is_control) || min >= max {
            return Err("A metric needs a name and an increasing integer range".into());
        }
        Ok(Self {
            name: name.trim().into(),
            min,
            max,
            ratings: BTreeMap::new(),
        })
    }

    pub fn validate(&self, value: i32) -> Result<()> {
        if !(self.min..=self.max).contains(&value) {
            return Err(format!(
                "{} must be between {} and {}",
                self.name, self.min, self.max
            )
            .into());
        }
        Ok(())
    }
}

enum Section {
    Text(String),
    Height,
    Weight,
    Exercise,
    Metric(String),
    Events,
}

pub struct Log {
    pub entries: BTreeMap<NaiveDate, Entry>,
    pub workouts: BTreeMap<NaiveDate, Vec<Workout>>,
    pub metrics: BTreeMap<String, Metric>,
    pub events: BTreeMap<NaiveDate, Vec<String>>,
    pub height: Option<f64>,
    layout: Vec<Section>,
}

pub fn calories(s: &str) -> Result<u32> {
    s.parse::<u32>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| "Exercise kcal must be a positive whole number".into())
}

fn cells(line: &str) -> Result<Vec<String>> {
    let line = line.trim();
    if line.len() < 2 || !line.starts_with('|') || !line.ends_with('|') {
        return Err("Table rows must start and end with |".into());
    }
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = line[1..line.len() - 1].chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some('|' | '\\')) => cell.push(chars.next().unwrap()),
            '|' => {
                cells.push(cell.trim().to_string());
                cell.clear();
            }
            _ => cell.push(c),
        }
    }
    cells.push(cell.trim().to_string());
    Ok(cells)
}

fn escape(note: &str) -> String {
    note.replace('\\', "\\\\").replace('|', "\\|")
}

impl Log {
    pub fn read(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(s) => Self::parse(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::parse("# Weight log\n\n"),
            Err(e) => Err(e.into()),
        }
    }

    fn parse(text: &str) -> Result<Self> {
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let mut height = None;
        let mut entries = BTreeMap::new();
        let mut workouts: BTreeMap<NaiveDate, Vec<Workout>> = BTreeMap::new();
        let mut layout = Vec::new();
        let mut metrics = BTreeMap::new();
        let mut events: BTreeMap<NaiveDate, Vec<String>> = BTreeMap::new();
        let mut pending_metric = None;
        let mut found_events = false;
        let mut found_weight = false;
        let mut found_exercise = false;
        let mut other_table = false;
        let mut start = 0;
        while start < lines.len() {
            if let Some(h) = lines[start].strip_prefix("Height:") {
                if height.is_some() {
                    return Err("Multiple Height settings in log".into());
                }
                let value = h
                    .trim()
                    .strip_suffix("cm")
                    .ok_or("Height must be written as Height: 171 cm")?
                    .trim();
                height = Some(positive(value, "Height in centimeters", 300.0)?);
                layout.push(Section::Height);
                start += 1;
                continue;
            }
            let header = cells(&lines[start]).unwrap_or_default();
            if pending_metric.is_some() && lines[start].trim_start().starts_with('#') {
                return Err("Missing metric table before next heading".into());
            }
            if let Some(declaration) = lines[start].strip_prefix("## Metric: ") {
                let (code, detail) = declaration
                    .split_once(" — ")
                    .ok_or("Invalid metric heading")?;
                let (name, range) = detail
                    .trim_end()
                    .strip_suffix(')')
                    .and_then(|s| s.rsplit_once(" ("))
                    .ok_or("Missing metric range")?;
                let (min, max) = range
                    .split_once('–')
                    .ok_or("Expected metric range MIN–MAX")?;
                let metric = Metric::new(code, name, min.parse()?, max.parse()?)?;
                if metrics.insert(code.to_owned(), metric).is_some() {
                    return Err(format!("Duplicate metric code: {code}").into());
                }
                pending_metric = Some(code.to_owned());
                layout.push(Section::Text(lines[start].clone()));
                start += 1;
                continue;
            }
            let section = if pending_metric.is_some() && header == ["Date", "Value", "Note"] {
                Section::Metric(pending_metric.take().unwrap())
            } else if header == ["Date", "Weight (kg)", "Note"] {
                if found_weight {
                    return Err("Multiple weight tables in log".into());
                }
                found_weight = true;
                Section::Weight
            } else if header == ["Date", "Active kcal", "Description"] {
                if found_exercise {
                    return Err("Multiple exercise tables in log".into());
                }
                found_exercise = true;
                Section::Exercise
            } else if header == ["Date", "Event"] {
                if found_events {
                    return Err("Multiple event tables in log".into());
                }
                found_events = true;
                Section::Events
            } else {
                other_table |= lines[start].trim_start().starts_with('|');
                layout.push(Section::Text(lines[start].clone()));
                start += 1;
                continue;
            };
            if pending_metric.is_some() {
                return Err("Missing metric table".into());
            }
            let columns = header.len();
            let sep = lines
                .get(start + 1)
                .ok_or("Missing Markdown table separator")?;
            let sep = cells(sep)?;
            if sep.len() != columns
                || sep.iter().any(|c| {
                    let s = c.trim_matches(':');
                    s.len() < 3 || !s.chars().all(|c| c == '-')
                })
            {
                return Err("Invalid Markdown table separator".into());
            }
            let mut end = start + 2;
            while end < lines.len() && lines[end].trim_start().starts_with('|') {
                let row = cells(&lines[end])?;
                if row.len() != columns {
                    return Err(format!(
                        "Line {}: expected {columns} columns; escape note pipes as \\|",
                        end + 1
                    )
                    .into());
                }
                let date = NaiveDate::parse_from_str(&row[0], "%Y-%m-%d")
                    .map_err(|e| format!("Line {}: invalid date: {e}", end + 1))?;
                if row.last().unwrap().chars().any(char::is_control) {
                    return Err("Notes cannot contain control characters".into());
                }
                if let Section::Metric(code) = &section {
                    let metric = metrics.get_mut(code).unwrap();
                    let value = row[1].parse()?;
                    metric.validate(value)?;
                    if metric
                        .ratings
                        .insert(date, (value, row[2].clone()))
                        .is_some()
                    {
                        return Err(format!("Duplicate {code} rating for {date}").into());
                    }
                } else if matches!(section, Section::Events) {
                    if row[1].is_empty() {
                        return Err("Missing event label".into());
                    }
                    events.entry(date).or_default().push(row[1].clone());
                } else if matches!(section, Section::Exercise) {
                    workouts.entry(date).or_default().push(Workout {
                        kcal: calories(&row[1])?,
                        note: row[2].clone(),
                    });
                } else if entries
                    .insert(
                        date,
                        Entry {
                            weight: positive(&row[1], "Weight in kilograms", 1000.0)?,
                            note: row[2].clone(),
                        },
                    )
                    .is_some()
                {
                    return Err(format!("Duplicate date {date}; keep one entry per day").into());
                }
                end += 1;
            }
            layout.push(section);
            start = end;
        }
        if pending_metric.is_some() {
            return Err("Missing metric table".into());
        }
        if !found_weight && !found_exercise && !found_events && metrics.is_empty() && other_table {
            return Err("Unrecognized table in log".into());
        }
        if !found_weight {
            while matches!(layout.last(), Some(Section::Text(s)) if s.is_empty()) {
                layout.pop();
            }
            if !layout.is_empty() {
                layout.push(Section::Text(String::new()));
            }
            layout.push(Section::Weight);
        }
        Ok(Self {
            entries,
            workouts,
            metrics,
            events,
            height,
            layout,
        })
    }

    pub fn define_metric(&mut self, code: String, metric: Metric) -> Result<()> {
        if self.metrics.contains_key(&code) {
            return Err(format!("Metric {code} already exists").into());
        }
        self.layout.push(Section::Text(format!(
            "\n## Metric: {code} — {} ({}–{})\n",
            metric.name, metric.min, metric.max
        )));
        self.layout.push(Section::Metric(code.clone()));
        self.metrics.insert(code, metric);
        Ok(())
    }

    fn write_events(&self, s: &mut String) {
        s.push_str("| Date       | Event |\n|------------|-------|\n");
        for (date, labels) in &self.events {
            for label in labels {
                s.push_str(&format!("| {date} | {} |\n", escape(label)));
            }
        }
    }

    fn markdown(&self) -> String {
        let mut s = String::new();
        let mut exercise_written = false;
        for section in &self.layout {
            match section {
                Section::Text(line) => {
                    s.push_str(line);
                    s.push('\n');
                }
                Section::Height => {
                    if let Some(h) = self.height {
                        s.push_str(&format!("Height: {h:.2} cm\n"));
                    }
                }
                Section::Events => self.write_events(&mut s),
                Section::Metric(code) => write_table(
                    &mut s,
                    "Value",
                    "Note",
                    self.metrics[code]
                        .ratings
                        .iter()
                        .map(|(date, (value, note))| (date, value.to_string(), escape(note))),
                ),
                Section::Weight => {
                    if let Some(h) = self
                        .height
                        .filter(|_| !self.layout.iter().any(|s| matches!(s, Section::Height)))
                    {
                        s.push_str(&format!("Height: {h:.2} cm\n\n"));
                    }
                    self.write_weights(&mut s);
                }
                Section::Exercise => {
                    self.write_exercise(&mut s);
                    exercise_written = true;
                }
            }
        }
        if !self.events.is_empty() && !self.layout.iter().any(|s| matches!(s, Section::Events)) {
            s.push_str("\n## Events\n\n");
            self.write_events(&mut s);
        }
        if !exercise_written && !self.workouts.is_empty() {
            s.push_str("\n## Exercise\n\n");
            self.write_exercise(&mut s);
        }
        s
    }

    fn write_exercise(&self, s: &mut String) {
        let rows = self.workouts.iter().flat_map(|(date, sessions)| {
            sessions
                .iter()
                .map(move |w| (date, w.kcal.to_string(), escape(&w.note)))
        });
        write_table(s, "Active kcal", "Description", rows);
    }

    fn write_weights(&self, s: &mut String) {
        let rows = self
            .entries
            .iter()
            .map(|(date, e)| (date, format!("{:.2}", e.weight), escape(&e.note)));
        write_table(s, "Weight (kg)", "Note", rows);
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        if let Ok(meta) = fs::metadata(path) {
            tmp.as_file().set_permissions(meta.permissions())?;
        }
        tmp.write_all(self.markdown().as_bytes())?;
        tmp.as_file().sync_all()?;
        tmp.persist(path)?;
        Ok(())
    }
}

fn write_table<'a>(
    s: &mut String,
    value: &str,
    description: &str,
    rows: impl IntoIterator<Item = (&'a NaiveDate, String, String)>,
) {
    let rows: Vec<_> = rows.into_iter().collect();
    let note_width = rows
        .iter()
        .map(|(_, _, note)| note.chars().count())
        .max()
        .unwrap_or(0)
        .max(description.len());
    s.push_str(&format!(
        "| {:<10} | {:>11} | {:<note_width$} |\n",
        "Date", value, description
    ));
    s.push_str(&format!(
        "|------------|------------:|{}|\n",
        "-".repeat(note_width + 2)
    ));
    for (date, weight, note) in rows {
        s.push_str(&format!(
            "| {date} | {weight:>11} | {note:<note_width$} |\n"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_escapes_and_preserves_prose_and_height() {
        let mut log = Log::parse("# My log\n\nPersonal notes.\n").unwrap();
        log.height = Some(171.0);
        log.layout
            .extend(["", "## Other notes", "Keep me."].map(|s| Section::Text(s.into())));
        let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
        log.entries.insert(
            date,
            Entry {
                weight: 83.5,
                note: r"a | b \ path".into(),
            },
        );
        let text = log.markdown();
        assert!(text.contains("Height: 171.00 cm\n"));
        assert!(text.contains("| 2026-09-29 |       83.50 | a \\| b \\\\ path |\n"));
        let widths: Vec<_> = text
            .lines()
            .filter(|line| line.starts_with('|'))
            .map(str::len)
            .collect();
        assert!(widths.windows(2).all(|pair| pair[0] == pair[1]));
        let parsed = Log::parse(&text).unwrap();
        assert_eq!(parsed.entries[&date].note, r"a | b \ path");
        assert_eq!(parsed.height, Some(171.0));
        assert_eq!(parsed.markdown(), text);
        assert!(text.contains("Personal notes."));
        assert!(text.ends_with("Keep me.\n"));
    }
    #[test]
    fn rejects_corruption_before_overwriting() {
        for rows in [
            "| 2026-01-01 | NaN | |",
            "| 2026-02-30 | 80 | |",
            "| 2026-01-01 | 80 | bad | pipe |",
            "| 2026-01-01 | 80 | |\n| 2026-01-01 | 81 | |",
        ] {
            assert!(Log::parse(&format!("{HEADER}\n|---|---|---|\n{rows}\n")).is_err());
        }
    }
}
