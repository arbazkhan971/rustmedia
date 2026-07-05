//! End-to-end tests that drive the compiled `rustmedia` binary.

use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/generated")
        .join(name);
    path.exists().then_some(path)
}

fn run(args: &[&str]) -> (String, String, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_rustmedia"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("run rustmedia binary");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

#[test]
fn inspect_human_output() {
    let Some(path) = fixture("av_2s.mp4") else {
        return;
    };
    let (stdout, _stderr, ok) = run(&["inspect", path.to_str().unwrap()]);
    assert!(ok, "inspect should succeed");
    assert!(
        stdout.contains("h264"),
        "should name the video codec:\n{stdout}"
    );
    assert!(stdout.contains("aac"), "should name the audio codec");
    assert!(stdout.contains("320×240"), "should show dimensions");
    assert!(
        stdout.contains("RustMedia Test"),
        "should show title metadata"
    );
}

#[test]
fn inspect_json_is_valid() {
    let Some(path) = fixture("av_2s.mp4") else {
        return;
    };
    let (stdout, _stderr, ok) = run(&["inspect", "--json", path.to_str().unwrap()]);
    assert!(ok);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(value["format"], "mp4");
    assert_eq!(value["streams"].as_array().unwrap().len(), 2);
    assert!((value["duration_secs"].as_f64().unwrap() - 2.0).abs() < 0.05);
}

#[test]
fn inspect_missing_file_fails_cleanly() {
    let (_stdout, stderr, ok) = run(&["inspect", "/no/such/file.mp4"]);
    assert!(!ok, "should exit non-zero");
    assert!(
        stderr.contains("error:"),
        "should print an error line:\n{stderr}"
    );
}

#[test]
fn help_and_version_work() {
    let (stdout, _e, ok) = run(&["--help"]);
    assert!(ok);
    assert!(stdout.contains("inspect"));

    let (stdout, _e, ok) = run(&["--version"]);
    assert!(ok);
    assert!(stdout.contains("rustmedia"));
}
