//! Safe handling of untrusted text: control characters and file reads.
//!
//! A document can contain terminal escape sequences (ESC, CSI, OSC, DCS, C1 controls)
//! and bidi overrides. None of them may reach the terminal or the clipboard as they are.

use std::borrow::Cow;
use std::io::Read;
use std::path::Path;

/// Files larger than this are not opened.
pub const MAX_FILE_BYTES: u64 = 32 << 20;

/// Characters that change how text is shown: bidi overrides, embeddings, isolates and marks.
fn is_bidi(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}')
}

/// For one line on screen: a tab becomes a space, ESC becomes a visible `␛`, other
/// control characters and bidi controls become `�`.
pub fn clean_line(s: &str) -> Cow<'_, str> {
    if !s.chars().any(|c| c.is_control() || is_bidi(c)) {
        return Cow::Borrowed(s);
    }
    Cow::Owned(
        s.chars()
            .map(|c| match c {
                '\t' => ' ',
                '\x1b' => '␛',
                c if c.is_control() || is_bidi(c) => '\u{FFFD}',
                c => c,
            })
            .collect(),
    )
}

/// For copied text: keeps new lines and tabs, drops all other control and bidi characters
/// (an ESC in a pasted command could end bracketed paste and run the rest).
pub fn clean_block(s: &str) -> String {
    s.chars().filter(|&c| c == '\n' || c == '\t' || !(c.is_control() || is_bidi(c))).collect()
}

/// A document read from disk or stdin.
pub struct Loaded {
    pub text: String,
    /// The bytes were not valid UTF-8; bad bytes are shown as `�`.
    pub lossy: bool,
}

/// Decodes bytes as UTF-8 (lossy) and drops a byte order mark.
pub fn decode(bytes: Vec<u8>) -> Loaded {
    let (text, lossy) = match String::from_utf8(bytes) {
        Ok(s) => (s, false),
        Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), true),
    };
    let text = match text.strip_prefix('\u{FEFF}') {
        Some(rest) => rest.to_string(),
        None => text,
    };
    Loaded { text, lossy }
}

/// Reads a regular file of at most `MAX_FILE_BYTES`. Devices, FIFOs and folders are refused,
/// so a link to `/dev/zero` or a named pipe cannot fill the memory or block.
pub fn read_file(path: &Path) -> std::io::Result<Loaded> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    read_limited(std::fs::File::open(path)?)
}

/// Reads at most `MAX_FILE_BYTES` from a reader (stdin).
pub fn read_limited(r: impl Read) -> std::io::Result<Loaded> {
    let mut buf = Vec::new();
    r.take(MAX_FILE_BYTES + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_FILE_BYTES {
        return Err(std::io::Error::other(format!("larger than {} MB", MAX_FILE_BYTES >> 20)));
    }
    Ok(decode(buf))
}

/// True for a regular file (not a device, FIFO or folder).
pub fn is_regular_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_escapes_controls_and_bidi() {
        assert_eq!(clean_line("plain"), "plain");
        assert!(matches!(clean_line("plain"), Cow::Borrowed(_)));
        assert_eq!(clean_line("a\x1b]52;c;eA==\x07b"), "a␛]52;c;eA==\u{FFFD}b");
        assert_eq!(clean_line("x\u{9b}31m\ty"), "x\u{FFFD}31m y");
        assert_eq!(clean_line("ab\u{202E}gpj.exe"), "ab\u{FFFD}gpj.exe");
        assert_eq!(clean_block("echo safe\x1b[201~\nrm\t-rf\r"), "echo safe[201~\nrm\t-rf");
    }

    #[test]
    fn decodes_lossy_and_drops_bom() {
        let l = decode(b"caf\xe9".to_vec());
        assert!(l.lossy);
        assert_eq!(l.text, "caf\u{FFFD}");
        let l = decode("\u{FEFF}---\ntitle: x\n---\n".as_bytes().to_vec());
        assert!(!l.lossy);
        assert!(l.text.starts_with("---"));
    }

    #[test]
    fn refuses_devices_and_large_input() {
        #[cfg(unix)]
        assert!(read_file(Path::new("/dev/zero")).is_err());
        assert!(read_file(Path::new("src")).is_err());
        let big = std::io::repeat(b'a').take(MAX_FILE_BYTES + 10);
        assert!(read_limited(big).is_err());
        assert_eq!(read_limited(&b"ok"[..]).unwrap().text, "ok");
    }
}
