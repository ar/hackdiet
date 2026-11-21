use chrono::{Duration, Local};
use std::{
    fs,
    process::{Command, Output},
};

fn run(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hackdiet"))
        .current_dir(dir)
        .env_remove("HACKDIET_FILE")
        .env("TERM_PROGRAM", "ghostty")
        .args(args)
        .output()
        .unwrap()
}

fn ok(dir: &std::path::Path, args: &[&str]) -> String {
    let output = run(dir, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        !text.contains('\x1b'),
        "redirected output must be plain text"
    );
    text
}

#[test]
fn daily_workflow_updates_in_place_and_recomputes_history() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    ok(dir, &["--no-graph", "height", "171"]);
    let yesterday = (Local::now().date_naive() - Duration::days(1)).to_string();
    ok(
        dir,
        &["--no-graph", "--date", &yesterday, "84", "first | note"],
    );
    let text = ok(dir, &["--no-graph", "83.5", "after a walk"]);
    assert!(text.contains("Weekly loss         0.35 kg/week"));
    assert!(text.contains("Estimated deficit   386 kcal/day"));
    assert!(text.contains("BMI                 "));
    let text = ok(dir, &["--no-graph", "83.0"]);
    assert!(text.contains("Updated"));
    assert!(text.contains("Estimated deficit   772 kcal/day"));
    let log = fs::read_to_string(dir.join("hackdiet.md")).unwrap();
    assert_eq!(log.lines().filter(|l| l.starts_with("| 20")).count(), 2);
    assert!(log.contains("after a walk"));
    assert!(log.contains("first \\| note"));
    ok(dir, &["--no-graph", "83.0", ""]);
    assert!(!fs::read_to_string(dir.join("hackdiet.md"))
        .unwrap()
        .contains("after a walk"));
    let before = fs::read(dir.join("hackdiet.md")).unwrap();
    assert!(!run(dir, &["NaN"]).status.success());
    assert_eq!(fs::read(dir.join("hackdiet.md")).unwrap(), before);
}

#[test]
fn png_export_and_single_entry_work_without_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    let text = ok(dir, &["--png", "chart.png", "83.5"]);
    assert!(text.contains("At least two weigh-ins"));
    let png = fs::read(dir.join("chart.png")).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 1200);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 600);
    ok(dir, &["--no-graph", "height", "171"]);
    ok(dir, &["--png", "bmi.png"]);
    let bmi_png = fs::read(dir.join("bmi.png")).unwrap();
    assert_ne!(png, bmi_png, "saved height should add the BMI axis");
    let before = fs::read(dir.join("hackdiet.md")).unwrap();
    assert!(!run(dir, &["--png", "./hackdiet.md"]).status.success());
    assert_eq!(fs::read(dir.join("hackdiet.md")).unwrap(), before);
}

#[test]
fn refuses_output_alias_before_creating_log_and_leaves_bad_files_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    assert!(!run(dir, &["--png", "./hackdiet.md", "83.5"])
        .status
        .success());
    assert!(!dir.join("hackdiet.md").exists());
    let bad = "# Notes\n\n| Date | Weight (kg) | Note |\n|---|---|---|\n| oops | 80 | |\n";
    fs::write(dir.join("hackdiet.md"), bad).unwrap();
    assert!(!run(dir, &["83.5"]).status.success());
    assert_eq!(fs::read_to_string(dir.join("hackdiet.md")).unwrap(), bad);
}

#[test]
fn read_only_empty_log_does_not_create_files() {
    let dir = tempfile::tempdir().unwrap();
    assert!(ok(dir.path(), &[]).contains("No weight entries"));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn workouts_append_independently_of_weight_and_render_in_both_views() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let text = ok(
        dir,
        &["--png", "exercise.png", "exercise", "300", r"gym | A\B"],
    );
    assert!(text.contains("No weight entries"));
    assert!(text.contains("1 workout · 300 kcal logged · 43 kcal/day"));
    assert!(dir.join("exercise.png").exists());

    let yesterday = (Local::now().date_naive() - Duration::days(1)).to_string();
    ok(dir, &["--date", &yesterday, "84"]);
    let baseline = ok(dir, &["83"]);
    let deficit = baseline
        .lines()
        .find(|s| s.starts_with("Estimated deficit"))
        .unwrap();
    let text = ok(
        dir,
        &["--png", "combined.png", "exercise", "600", "10k", "run"],
    );
    assert!(text.contains(deficit));
    assert!(text.contains("2 workouts · 900 kcal logged · 129 kcal/day"));
    let chart = fs::read(dir.join("combined.png")).unwrap();
    assert_eq!(u32::from_be_bytes(chart[20..24].try_into().unwrap()), 840);
    let log = fs::read_to_string(dir.join("hackdiet.md")).unwrap();
    assert!(log.contains(r"gym \| A\\B"));
    assert!(log.contains("10k run"));
    for kcal in ["0", "-1", "NaN", ""] {
        assert!(!run(dir, &["exercise", kcal]).status.success());
    }
    assert_eq!(log, fs::read_to_string(dir.join("hackdiet.md")).unwrap());
}

#[test]
fn personal_scales_and_events_share_a_timeline_without_weigh_ins() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let yesterday = (Local::now().date_naive() - Duration::days(1)).to_string();
    ok(dir, &["metric", "define", "en", "0", "5", "Energy"]);
    ok(dir, &["metric", "define", "ws", "0", "5", "Work stress"]);
    ok(dir, &["metric", "define", "load", "-2", "10", "Workload"]);
    ok(
        dir,
        &["--date", &yesterday, "metric", "en", "2", "late | shift"],
    );
    ok(dir, &["--date", &yesterday, "metric", "en", "0"]);
    ok(dir, &["metric", "ws", "5"]);
    ok(dir, &["metric", "load", "-2"]);
    ok(dir, &["event", "Release | day"]);
    ok(dir, &["event", "First shift"]);
    let text = ok(dir, &["--png", "timeline.png"]);
    for label in [
        "Work stress",
        "Energy",
        "Workload",
        "Release | day",
        "First shift",
        &format!("0 ({yesterday})"),
    ] {
        assert!(text.contains(label));
    }
    let png = fs::read(dir.join("timeline.png")).unwrap();
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 676);
    let file = dir.join("hackdiet.md");
    let saved = fs::read_to_string(&file).unwrap();
    assert!(saved.contains(r"late \| shift"));
    assert_eq!(saved.matches(&yesterday).count(), 1);
    for args in [
        vec!["metric", "en", "6"],
        vec!["metric", "xx", "3"],
        vec!["metric", "define", "en", "0", "9", "other"],
        vec!["metric", "define", "bad", "5", "1", "bad"],
        vec!["event", ""],
    ] {
        assert!(!run(dir, &args).status.success());
    }
    assert_eq!(fs::read_to_string(&file).unwrap(), saved);
    let corrupt = saved.replace("Energy (0–5)", "Energy (1–5)");
    fs::write(&file, &corrupt).unwrap();
    assert!(!run(dir, &["exercise", "100"]).status.success());
    assert_eq!(fs::read_to_string(file).unwrap(), corrupt);
}
