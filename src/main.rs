mod chart;
mod log;
mod stats;

use chrono::{Datelike, Local, Months, NaiveDate};
use fs2::FileExt;
use std::{env, error::Error, fs::OpenOptions, io::IsTerminal, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const HELP: &str = "hackdiet — daily weight in kilograms, stored in Markdown

Usage:
  hackdiet 83.5 [note words...]   Record today's weight, replacing today's entry
  hackdiet                      Show the latest seven calendar days
  hackdiet this [N]              Current month and N−1 preceding months
  hackdiet last [N]              N complete months before this month
  hackdiet height 171            Save height in centimeters for BMI

Options (before the weight/note, or with a viewing command):
  --file PATH                   Log file (default: HACKDIET_FILE or ./hackdiet.md)
  --date YYYY-MM-DD              Record a past date instead of today
  --png PATH                    Also export the chart as PNG
  --no-graph                    Only print the summary
  --kitty                       Force Kitty graphics on a terminal
  -h, --help                    Show this help

Ghostty and Kitty display the graph inline. Other terminals show a small table.
Redirected output is plain text. Height is optional; weight logging works without it.";

#[derive(Debug)]
enum Action {
    Show(Option<(bool, u32)>),
    Record(f64, Option<String>),
    Height(f64),
}

struct Options {
    file: PathBuf,
    date: Option<NaiveDate>,
    png: Option<PathBuf>,
    no_graph: bool,
    kitty: bool,
    action: Action,
}

fn positive(s: &str, label: &str, max: f64) -> Result<f64> {
    let n: f64 = s.parse().map_err(|_| format!("Invalid {label}: {s}"))?;
    if !n.is_finite() || n <= 0.0 || n > max {
        return Err(format!("{label} must be greater than 0 and at most {max}").into());
    }
    Ok(n)
}

fn parse(args: Vec<String>) -> Result<Options> {
    let mut opt = Options {
        file: env::var_os("HACKDIET_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| "hackdiet.md".into()),
        date: None,
        png: None,
        no_graph: false,
        kitty: false,
        action: Action::Show(None),
    };
    let mut words = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--file" | "--date" | "--png" => {
                let flag = &args[i];
                i += 1;
                let value = args.get(i).ok_or_else(|| format!("{flag} needs a value"))?;
                match flag.as_str() {
                    "--file" => opt.file = value.into(),
                    "--png" => opt.png = Some(value.into()),
                    _ => opt.date = Some(NaiveDate::parse_from_str(value, "%Y-%m-%d")?),
                }
            }
            "--no-graph" => opt.no_graph = true,
            "--kitty" => opt.kitty = true,
            s if words.is_empty() && s.parse::<f64>().is_ok() => {
                let weight = positive(s, "Weight in kilograms", 1000.0)?;
                let note = (i + 1 < args.len()).then(|| args[i + 1..].join(" "));
                if note
                    .as_ref()
                    .is_some_and(|s| s.chars().any(char::is_control))
                {
                    return Err("Notes cannot contain newlines or control characters".into());
                }
                opt.action = Action::Record(weight, note);
                return Ok(opt);
            }
            s if s.starts_with('-') => return Err(format!("Unknown option: {s}").into()),
            _ => words.push(args[i].clone()),
        }
        i += 1;
    }
    opt.action = match words.first().map(String::as_str) {
        None => Action::Show(None),
        Some("this" | "last") if words.len() <= 2 => {
            let n = words
                .get(1)
                .map(|v| v.parse::<u32>())
                .transpose()?
                .unwrap_or(1);
            if n == 0 || n > 1200 {
                return Err("Month count must be between 1 and 1200".into());
            }
            Action::Show(Some((words[0] == "this", n)))
        }
        Some("height") if words.len() == 2 => {
            Action::Height(positive(&words[1], "Height in centimeters", 300.0)?)
        }
        _ => return Err("Expected a weight, this [N], last [N], or height CM; see --help".into()),
    };
    if opt.date.is_some() {
        return Err("--date requires a weight entry".into());
    }
    if opt.no_graph && opt.png.is_some() {
        return Err("--png and --no-graph cannot be combined".into());
    }
    Ok(opt)
}

fn period(today: NaiveDate, selection: Option<(bool, u32)>) -> (NaiveDate, NaiveDate) {
    let first = today.with_day(1).unwrap();
    match selection {
        None => (today - chrono::Duration::days(6), today),
        Some((true, n)) => (first.checked_sub_months(Months::new(n - 1)).unwrap(), today),
        Some((false, n)) => (
            first.checked_sub_months(Months::new(n)).unwrap(),
            first.pred_opt().unwrap(),
        ),
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|s| matches!(s.as_str(), "--help" | "-h"))
    {
        println!("{HELP}");
        return Ok(());
    }
    let opt = parse(args)?;
    if opt.no_graph && opt.png.is_some() {
        return Err("--png and --no-graph cannot be combined".into());
    }
    let today = Local::now().date_naive();
    let date = opt.date.unwrap_or(today);
    if date > today {
        return Err("Cannot record a future weight".into());
    }
    // Resolve existing symlinks so atomic replacement updates their target.
    let file = resolve_path(&opt.file)?;
    if let Some(png) = &opt.png {
        if resolve_path(png)? == file {
            return Err("PNG output cannot overwrite the weight log".into());
        }
    }
    let mutating = !matches!(opt.action, Action::Show(_));
    let _lock = if mutating {
        let name = file
            .file_name()
            .ok_or("Invalid log path")?
            .to_string_lossy();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(file.with_file_name(format!(".{name}.lock")))?;
        lock.lock_exclusive()?;
        Some(lock)
    } else {
        None
    };
    let mut log = log::Log::read(&file)?;
    if log.entries.keys().any(|d| *d > today) {
        return Err("The log contains a future weight; correct it before continuing".into());
    }
    let selection = match opt.action {
        Action::Record(weight, note) => {
            let replaced = log.entries.contains_key(&date);
            let note = note.unwrap_or_else(|| {
                log.entries
                    .get(&date)
                    .map(|e| e.note.clone())
                    .unwrap_or_default()
            });
            log.entries.insert(date, log::Entry { weight, note });
            log.save(&file)?;
            println!(
                "{} {date}: {weight} kg.",
                if replaced { "Updated" } else { "Recorded" }
            );
            None
        }
        Action::Height(height) => {
            log.height = Some(height);
            log.save(&file)?;
            println!("Height saved: {height} cm.");
            None
        }
        Action::Show(p) => p,
    };
    drop(_lock);
    let (start, end) = period(date, selection);
    let days = stats::days(&log.entries);
    let selected: Vec<_> = days
        .into_iter()
        .filter(|d| d.date >= start && d.date <= end)
        .collect();
    println!("{start} to {end}");
    stats::print_summary(&selected, log.height);
    if !opt.no_graph && !selected.is_empty() {
        let graphics = std::io::stdout().is_terminal() && (opt.kitty || chart::supported());
        if graphics || opt.png.is_some() {
            let image = tempfile::Builder::new().suffix(".png").tempfile()?;
            chart::render(image.path(), &selected, start, end)?;
            if let Some(path) = opt.png {
                let path = resolve_path(&path)?;
                let export = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
                std::fs::copy(image.path(), export.path())?;
                export.persist(&path)?;
                println!("Chart saved: {}", path.display());
            }
            if graphics {
                chart::display(&std::fs::read(image.path())?)?;
            }
        }
        if !graphics {
            println!("\nDate          Weight    Trend");
            for day in selected.iter().filter(|d| d.weight.is_some()) {
                println!(
                    "{}    {:6.2}   {:6.2}",
                    day.date,
                    day.weight.unwrap(),
                    day.trend
                );
            }
        }
    }
    Ok(())
}

fn resolve_path(path: &std::path::Path) -> Result<PathBuf> {
    if std::fs::symlink_metadata(path).is_ok() {
        return Ok(path.canonicalize()?);
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    Ok(parent
        .canonicalize()?
        .join(path.file_name().ok_or("Invalid file path")?))
}

fn main() {
    if let Err(e) = run() {
        eprintln!("hackdiet: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calendar_periods_cross_years_and_leap_days() {
        let d = NaiveDate::from_ymd_opt(2024, 3, 2).unwrap();
        assert_eq!(period(d, None).0.to_string(), "2024-02-25");
        let (a, b) = period(d, Some((false, 3)));
        assert_eq!(
            (a.to_string(), b.to_string()),
            ("2023-12-01".into(), "2024-02-29".into())
        );
        assert_eq!(period(d, Some((true, 3))).0.to_string(), "2024-01-01");
    }
    #[test]
    fn cli_notes_are_literal_and_numbers_validated() {
        let p = parse(vec!["83.5".into(), "after".into(), "--run".into()]).unwrap();
        assert!(matches!(p.action, Action::Record(_, Some(s)) if s == "after --run"));
        for args in [
            vec!["NaN"],
            vec!["0"],
            vec!["-1"],
            vec!["last", "0"],
            vec!["last", "many"],
            vec!["height", "inf"],
            vec!["--file"],
        ] {
            assert!(parse(args.into_iter().map(String::from).collect()).is_err());
        }
    }
}
