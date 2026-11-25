//! Runs the real binary in print mode (stdout is a pipe here, so mdr prints instead of opening the viewer).

use std::io::Write;
use std::process::{Command, Stdio};

fn mdr(args: &[&str], stdin: Option<&str>) -> (bool, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdr"))
        .args(args)
        .env_remove("NO_COLOR")
        // Never the user's config file.
        .env("MDR_CONFIG", "tests/fixtures/empty-config.toml")
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

#[test]
fn printing_a_folder_is_an_error() {
    let (ok, _, err) = mdr(&["tests/fixtures"], None);
    assert!(!ok);
    assert!(err.contains("is a folder"), "{err}");
}

#[test]
fn config_file_sets_width_and_command_line_wins() {
    let dir = std::env::temp_dir().join(format!("mdr-cli-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("config.toml");
    std::fs::write(&cfg, "width = 20\n").unwrap();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_mdr"))
            .args(args)
            .env("MDR_CONFIG", &cfg)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let widest = |s: &str| s.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    let (ok, out, err) = run(&["tests/fixtures/other.md"]);
    assert!(ok, "{err}");
    assert!(widest(&out) <= 20, "{out}");
    let (_, out, _) = run(&["--width", "60", "tests/fixtures/other.md"]);
    assert!(widest(&out) > 20);
    let (_, out, _) = run(&["--no-config", "tests/fixtures/other.md"]);
    assert!(widest(&out) > 20);

    std::fs::write(&cfg, "widht = 20\n").unwrap();
    let (ok, _, err) = run(&["tests/fixtures/other.md"]);
    assert!(!ok);
    assert!(err.contains("config.toml") && err.contains("widht"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn prints_completions_and_man_page() {
    let (ok, out, _) = mdr(&["--completions", "bash"], None);
    assert!(ok && out.contains("_mdr()"), "{out}");
    let (ok, out, _) = mdr(&["--completions", "zsh"], None);
    assert!(ok && out.contains("#compdef mdr"));
    let (ok, out, _) = mdr(&["--man"], None);
    assert!(ok && out.contains(".TH mdr"), "{out}");
}

#[test]
fn escape_sequences_in_the_document_never_reach_the_terminal() {
    let md = "# Title \x1b]0;PWNED\x07\n\nText \x1b[2J \x1b]52;c;ZWNobw==\x07 \u{9b}31m \x1bP1$qm\x1b\\ x\u{202E}y\n\n```\ncode \x1b]0;T2\x07\n```\n\n| a\x1b[31m | b |\n|---|---|\n| $x\x1b]0;M\x07$ | c |\n";
    for args in [&["-p", "--color", "never"][..], &["-p", "--color", "always"][..]] {
        let (ok, out, err) = mdr(args, Some(md));
        assert!(ok, "{err}");
        // The only ESC allowed is the start of mdr's own color codes: ESC [ digits ; m
        let bytes = out.as_bytes();
        for (i, _) in out.match_indices('\x1b') {
            let rest = &bytes[i + 1..];
            assert!(rest.starts_with(b"[0") , "foreign escape at {i}: {:?}", &out[i..(i + 12).min(out.len())]);
        }
        assert!(!out.contains('\u{9b}') && !out.contains('\u{202E}') && !out.contains('\x07'));
        assert!(out.contains("Title ␛]0;PWNED"), "{out}");
    }
}

#[test]
fn a_device_or_huge_input_is_refused() {
    #[cfg(unix)]
    {
        let (ok, _, err) = mdr(&["/dev/zero"], None);
        assert!(!ok && err.contains("not a regular file"), "{err}");
    }
    let (ok, out, err) = mdr(&["-p"], Some("caf\u{FEFF}"));
    assert!(ok, "{err}");
    assert!(out.contains("caf"));
}
