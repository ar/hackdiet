use crate::{positive, Result};
use chrono::NaiveDate;
use std::{collections::BTreeMap, fs, io::Write, path::Path};

const HEADER: &str = "| Date | Weight (kg) | Note |";

#[derive(Clone, Debug)]
pub struct Entry {
    pub weight: f64,
    pub note: String,
}

pub struct Log {
    pub entries: BTreeMap<NaiveDate, Entry>,
    pub height: Option<f64>,
    // Preserve prose surrounding the managed table.
    before: Vec<String>,
    after: Vec<String>,
}

fn cells(line: &str) -> Result<Vec<String>> {
    let line = line.trim();
    if !line.starts_with('|') || !line.ends_with('|') {
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
        for line in &lines {
            if let Some(h) = line.strip_prefix("Height:") {
                if height.is_some() {
                    return Err("Multiple Height settings in log".into());
                }
                let value = h
                    .trim()
                    .strip_suffix("cm")
                    .ok_or("Height must be written as Height: 171 cm")?
                    .trim();
                height = Some(positive(value, "Height in centimeters", 300.0)?);
            }
        }
        let headers: Vec<_> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| {
                cells(l)
                    .ok()
                    .filter(|c| c == &["Date", "Weight (kg)", "Note"])
                    .map(|_| i)
            })
            .collect();
        if headers.len() > 1 {
            return Err("Multiple weight tables in log".into());
        }
        let Some(&start) = headers.first() else {
            if lines.iter().any(|l| l.trim_start().starts_with('|')) {
                return Err(format!("Unrecognized table; expected {HEADER}").into());
            }
            return Ok(Self {
                entries: BTreeMap::new(),
                height,
                before: lines,
                after: Vec::new(),
            });
        };
        let sep = lines
            .get(start + 1)
            .ok_or("Missing Markdown table separator")?;
        let sep = cells(sep)?;
        if sep.len() != 3
            || sep.iter().any(|c| {
                let s = c.trim_matches(':');
                s.len() < 3 || !s.chars().all(|c| c == '-')
            })
        {
            return Err("Invalid Markdown table separator".into());
        }
        let mut end = start + 2;
        let mut entries = BTreeMap::new();
        while end < lines.len() && lines[end].trim_start().starts_with('|') {
            let row = cells(&lines[end])?;
            if row.len() != 3 {
                return Err(format!(
                    "Line {}: expected three columns; escape note pipes as \\|",
                    end + 1
                )
                .into());
            }
            let date = NaiveDate::parse_from_str(&row[0], "%Y-%m-%d")
                .map_err(|e| format!("Line {}: invalid date: {e}", end + 1))?;
            let weight = positive(&row[1], "Weight in kilograms", 1000.0)?;
            if row[2].chars().any(char::is_control) {
                return Err("Notes cannot contain control characters".into());
            }
            if entries
                .insert(
                    date,
                    Entry {
                        weight,
                        note: row[2].clone(),
                    },
                )
                .is_some()
            {
                return Err(format!("Duplicate date {date}; keep one entry per day").into());
            }
            end += 1;
        }
        Ok(Self {
            entries,
            height,
            before: lines[..start].to_vec(),
            after: lines[end..].to_vec(),
        })
    }

    fn markdown(&self) -> String {
        let mut before: Vec<_> = self
            .before
            .iter()
            .filter(|l| !l.starts_with("Height:"))
            .cloned()
            .collect();
        while before.last().is_some_and(|s| s.trim().is_empty()) {
            before.pop();
        }
        let mut s = before.join("\n");
        if !s.is_empty() {
            s.push_str("\n\n");
        }
        if let Some(h) = self.height {
            s.push_str(&format!("Height: {h} cm\n\n"));
        }
        s.push_str(HEADER);
        s.push_str("\n|------------|-------------|------|\n");
        for (date, e) in &self.entries {
            s.push_str(&format!(
                "| {date} | {} | {} |\n",
                e.weight,
                escape(&e.note)
            ));
        }
        for line in &self.after {
            if !line.starts_with("Height:") {
                s.push_str(line);
                s.push('\n');
            }
        }
        s
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_escapes_and_preserves_prose_and_height() {
        let mut log = Log::parse("# My log\n\nPersonal notes.\n").unwrap();
        log.height = Some(171.0);
        log.after = vec!["".into(), "## Other notes".into(), "Keep me.".into()];
        let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
        log.entries.insert(
            date,
            Entry {
                weight: 83.5,
                note: r"a | b \ path".into(),
            },
        );
        let text = log.markdown();
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
