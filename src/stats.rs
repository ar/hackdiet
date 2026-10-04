use crate::log::Entry;
use chrono::{Datelike, NaiveDate};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct Day {
    pub date: NaiveDate,
    pub weight: Option<f64>,
    pub trend: f64,
}

// Start with the first actual measurement. Carry the trend across missing days,
// without inventing weigh-ins or extending the series beyond the last observation.
pub fn days(entries: &BTreeMap<NaiveDate, Entry>) -> Vec<Day> {
    let Some((&first, entry)) = entries.first_key_value() else {
        return Vec::new();
    };
    let last = *entries.last_key_value().unwrap().0;
    let mut trend = entry.weight;
    let mut out = Vec::new();
    let mut date = first;
    loop {
        let weight = entries.get(&date).map(|e| e.weight);
        if let Some(w) = weight {
            trend += (w - trend) / 10.0;
        }
        out.push(Day {
            date,
            weight,
            trend,
        });
        if date == last {
            break;
        }
        date = date.succ_opt().unwrap();
    }
    out
}

pub fn slope(days: &[Day]) -> Option<f64> {
    if days.iter().filter(|d| d.weight.is_some()).count() < 2 {
        return None;
    }
    let first = days.first()?.date;
    let n = days.len() as f64;
    let mx = days
        .iter()
        .map(|d| (d.date - first).num_days() as f64)
        .sum::<f64>()
        / n;
    let my = days.iter().map(|d| d.trend).sum::<f64>() / n;
    let numerator: f64 = days
        .iter()
        .map(|d| ((d.date - first).num_days() as f64 - mx) * (d.trend - my))
        .sum();
    let denominator: f64 = days
        .iter()
        .map(|d| ((d.date - first).num_days() as f64 - mx).powi(2))
        .sum();
    (denominator > 0.0).then_some(numerator / denominator)
}

pub fn bmi(days: &[Day], height: f64) -> Option<(f64, f64)> {
    let recent = days.last()?.trend;
    let mean = days.iter().map(|d| d.trend).sum::<f64>() / days.len() as f64;
    let squared = (height / 100.0).powi(2);
    Some((mean / squared, recent / squared))
}

pub fn period_label(start: NaiveDate, end: NaiveDate) -> String {
    if start.year() != end.year() {
        format!("{}–{}", start.format("%d %b %Y"), end.format("%d %b %Y"))
    } else if start.month() != end.month() {
        format!("{}–{}", start.format("%d %b"), end.format("%d %b %Y"))
    } else {
        format!("{}–{}", start.format("%d"), end.format("%d %b %Y"))
    }
}

pub fn change_labels(days: &[Day]) -> Option<[(String, &'static str); 2]> {
    let slope = slope(days)?;
    let weekly = slope * 7.0;
    let calories = slope * 7716.0;
    Some(if weekly.abs() < 0.005 && calories.abs() < 0.5 {
        [
            ("0.00 kg/week".into(), "weight stable"),
            ("0 kcal/day".into(), "estimated balance"),
        ]
    } else {
        [
            (
                format!("{:.2} kg/week", weekly.abs()),
                if slope < 0.0 {
                    "weekly loss"
                } else {
                    "weekly gain"
                },
            ),
            (
                format!("{:.0} kcal/day", calories.abs()),
                if slope < 0.0 {
                    "estimated deficit"
                } else {
                    "estimated excess"
                },
            ),
        ]
    })
}

pub fn print_summary(days: &[Day], height: Option<f64>) {
    if days.is_empty() {
        println!("No weight entries in this period.");
        return;
    }
    if let Some(labels) = change_labels(days) {
        for (value, label) in labels {
            let label = format!("{}{}", label[..1].to_uppercase(), &label[1..]);
            println!("{label:<19} {value}");
        }
    } else {
        println!("At least two weigh-ins in this period are needed to estimate weight change and calorie balance.");
    }
    if let Some(height) = height {
        if let Some((mean, latest)) = bmi(days, height) {
            println!("{:<19} {latest:.1} (mean {mean:.1})", "BMI");
        }
    } else {
        println!("Set height for BMI: hackdiet height <centimeters>.");
    }
    let latest = days.last().unwrap();
    println!(
        "{:<19} {:.2} kg ({})",
        "Latest trend", latest.trend, latest.date
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smoothing_carries_across_gaps_and_months() {
        let mut entries = BTreeMap::new();
        for (d, w) in [
            ("2024-02-28", 80.0),
            ("2024-03-01", 79.0),
            ("2024-03-02", 78.0),
        ] {
            entries.insert(
                NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap(),
                Entry {
                    weight: w,
                    note: String::new(),
                },
            );
        }
        let d = days(&entries);
        assert_eq!(d.len(), 4);
        for (d, expected) in d.iter().zip([80.0, 80.0, 79.9, 79.71]) {
            assert!((d.trend - expected).abs() < 1e-10);
        }
        assert_eq!(d[1].weight, None);
        assert!((slope(&d).unwrap() + 0.097).abs() < 1e-10);
        let (mean, last) = bmi(&d, 200.0).unwrap();
        assert!((mean - 19.975625).abs() < 1e-10);
        assert!((last - 19.9275).abs() < 1e-10);
    }
    #[test]
    fn exact_linear_rate_and_insufficient_data() {
        let first = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let d: Vec<_> = (0..7)
            .map(|i| Day {
                date: first + chrono::Duration::days(i),
                weight: Some(80.0),
                trend: 80.0 - i as f64 * 0.02,
            })
            .collect();
        assert!((slope(&d).unwrap() * 7.0 + 0.14).abs() < 1e-10);
        assert!((slope(&d).unwrap() * 7716.0 + 154.32).abs() < 1e-8);
        assert!(slope(&d[..1]).is_none());
        assert!(slope(&[]).is_none());
    }
}
