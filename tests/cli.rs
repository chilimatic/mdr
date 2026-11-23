//! Runs the real binary in print mode (stdout is a pipe here, so mdr prints instead of opening the viewer).

use std::io::Write;
use std::process::{Command, Stdio};

fn mdr(args: &[&str], stdin: Option<&str>) -> (bool, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdr"))
        .args(args)
        .env_remove("NO_COLOR")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = stdin {
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into(), String::from_utf8_lossy(&out.stderr).into())
}

#[test]
fn piped_output_is_plain_text() {
    let (ok, out, err) = mdr(&["tests/fixtures/kitchen-sink.md"], None);
    assert!(ok, "{err}");
    assert!(out.starts_with("Kitchen Sink\n"), "{out}");
    assert!(out.contains("│ bananas │"));
    assert!(!out.contains('\x1b'));
}

#[test]
fn color_always_adds_escapes() {
    let (ok, out, _) = mdr(&["--color", "always", "tests/fixtures/kitchen-sink.md"], None);
    assert!(ok);
    assert!(out.contains("\x1b["));
}

#[test]
fn reads_stdin_and_respects_width() {
    let (ok, out, _) = mdr(&["--width", "20"], Some("one two three four five six seven eight nine ten"));
    assert!(ok);
    assert!(out.lines().count() > 2);
    assert!(out.lines().all(|l| l.chars().count() <= 20), "{out}");
}

#[test]
fn diagrams_print_as_text() {
    let (ok, out, _) = mdr(&["tests/fixtures/diagrams.md"], None);
    assert!(ok);
    assert!(out.contains("Cached?") && out.contains("▼"));
}

#[test]
fn watch_with_print_is_an_error() {
    let (ok, _, err) = mdr(&["--watch", "tests/fixtures/other.md"], None);
    assert!(!ok);
    assert!(err.contains("--watch"), "{err}");
}
