//! Copies text to the system clipboard.
//!
//! On macOS mdr uses `pbcopy`. With a graphical session on Linux, it uses `wl-copy`,
//! `xclip` or `xsel`. Otherwise (for
//! example over ssh) it asks the terminal with the OSC 52 escape sequence.

use std::io::Write;
use std::process::{Command, Stdio};

/// Copies `text`. Returns how it was copied, for the status message.
pub fn copy(text: &str) -> std::io::Result<&'static str> {
    let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    let mut tools: Vec<(&'static str, &[&str])> = Vec::new();
    if cfg!(target_os = "macos") {
        tools.push(("pbcopy", &[]));
    }
    if set("WAYLAND_DISPLAY") {
        tools.push(("wl-copy", &[]));
    }
    if set("DISPLAY") {
        tools.push(("xclip", &["-selection", "clipboard"]));
        tools.push(("xsel", &["--clipboard", "--input"]));
    }
    for (tool, args) in tools {
        let Ok(mut child) =
            Command::new(tool).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
        else {
            continue;
        };
        let written = child.stdin.take().map(|mut s| s.write_all(text.as_bytes()));
        if matches!(written, Some(Err(_))) {
            let _ = child.kill();
            let _ = child.wait();
            continue;
        }
        // A tool that cannot reach the display exits at once with an error.
        std::thread::sleep(std::time::Duration::from_millis(30));
        if let Ok(Some(status)) = child.try_wait() {
            if !status.success() {
                continue;
            }
        } else {
            // xclip and wl-copy keep running to serve the clipboard; reap them later.
            std::thread::spawn(move || child.wait());
        }
        return Ok(match tool {
            "wl-copy" => "via wl-copy",
            "xclip" => "via xclip",
            "pbcopy" => "via pbcopy",
            _ => "via xsel",
        });
    }
    let mut out = std::io::stdout();
    write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
    out.flush()?;
    Ok("sent to the terminal")
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn encodes_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64("héllo\n".as_bytes()), "aMOpbGxvCg==");
    }
}
