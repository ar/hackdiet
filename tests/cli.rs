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
    assert!(text.contains("Weekly loss 0.35 kilograms. Daily deficit: 386 calories."));
    assert!(text.contains("Body mass index:"));
    let text = ok(dir, &["--no-graph", "83.0"]);
    assert!(text.contains("Updated"));
    assert!(text.contains("Daily deficit: 772 calories."));
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
