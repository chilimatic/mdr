//! Text search across the files of a folder (`s` in the file list).

use std::path::{Path, PathBuf};

/// The search stops after this many matching lines.
pub const MAX_HITS: usize = 1000;
/// Larger files are skipped.
const MAX_FILE_BYTES: u64 = 4 << 20;
/// Longer lines are cut around the match.
const MAX_TEXT_CHARS: usize = 300;

pub struct Hit {
    /// Index into `Picker::files`.
    pub file: usize,
    /// Source line, counted from 1.
    pub line: usize,
    /// Byte offset of the line start in the file.
    pub offset: usize,
    /// The line, without leading whitespace, maybe cut.
    pub text: String,
    /// Char range of the match in `text`.
    pub start: usize,
    pub end: usize,
}

#[derive(Default)]
pub struct Grep {
    pub query: String,
    /// The query is being typed.
    pub editing: bool,
    pub hits: Vec<Hit>,
    /// The search stopped at `MAX_HITS`.
    pub truncated: bool,
    pub selected: usize,
    /// The query of the shown results, while a new one is typed. `Esc` goes back to it.
    pub previous: String,
}

impl Grep {
    pub fn files_hit(&self) -> usize {
        let mut n = 0;
        let mut last = None;
        for h in &self.hits {
            if last != Some(h.file) {
                n += 1;
                last = Some(h.file);
            }
        }
        n
    }
}

/// Finds `query` in the source text of `files`, one hit per line, in file order.
/// Smart case, like the search in a document. Returns the hits and whether the search stopped early.
pub fn search(root: &Path, files: &[PathBuf], query: &str) -> (Vec<Hit>, bool) {
    let mut hits = Vec::new();
    if query.is_empty() {
        return (hits, false);
    }
    let case_sensitive = query.chars().any(char::is_uppercase);
    let norm = |c: char| if case_sensitive { c } else { c.to_lowercase().next().unwrap_or(c) };
    let q: Vec<char> = query.chars().map(norm).collect();
    let q_str: String = q.iter().collect();
    for (fi, rel) in files.iter().enumerate() {
        let path = root.join(rel);
        // Only regular files: a symlink to /dev/zero reports size 0.
        if !std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_FILE_BYTES) {
            continue;
        }
        let Ok(text) = crate::text::read_file(&path).map(|l| l.text) else { continue };
        // Most files do not match: check the whole file once first.
        let found = if case_sensitive { text.contains(query) } else { text.chars().map(norm).collect::<String>().contains(&q_str) };
        if !found {
            continue;
        }
        let mut offset = 0;
        for (li, raw) in text.split('\n').enumerate() {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            // The terminal drops tabs, which would join words.
            let chars: Vec<char> = line.chars().map(|c| if c == '\t' { ' ' } else { c }).collect();
            let normed: Vec<char> = chars.iter().map(|&c| norm(c)).collect();
            if let Some(start) = normed.windows(q.len()).position(|w| w == q.as_slice()) {
                hits.push(snippet(fi, li + 1, offset, &chars, start, start + q.len()));
                if hits.len() >= MAX_HITS {
                    return (hits, true);
                }
            }
            offset += raw.len() + 1;
        }
    }
    (hits, false)
}

fn snippet(file: usize, line: usize, offset: usize, chars: &[char], start: usize, end: usize) -> Hit {
    let lead = chars.iter().take_while(|c| c.is_whitespace()).count().min(start);
    let mut from = lead;
    if end > from + MAX_TEXT_CHARS {
        from = start.saturating_sub(40).max(lead);
    }
    let to = chars.len().min(from + MAX_TEXT_CHARS);
    let mut text: String = chars[from..to].iter().collect();
    let mut shift = 0;
    if from > lead {
        text.insert(0, '…');
        shift = 1;
    }
    Hit { file, line, offset, text, start: start - from + shift, end: (end - from + shift).min(to - from + shift) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_lines_in_fixture_files() {
        let files = vec![PathBuf::from("kitchen-sink.md"), PathBuf::from("other.md")];
        let (hits, truncated) = search(Path::new("tests/fixtures"), &files, "bananas");
        assert!(!truncated);
        assert_eq!(hits.len(), 1);
        let h = &hits[0];
        assert_eq!(h.file, 0);
        let src = std::fs::read_to_string("tests/fixtures/kitchen-sink.md").unwrap();
        assert_eq!(src.lines().nth(h.line - 1).unwrap().trim_start(), h.text);
        assert!(src[h.offset..].starts_with(src.lines().nth(h.line - 1).unwrap()));
        let found: String = h.text.chars().skip(h.start).take(h.end - h.start).collect();
        assert_eq!(found, "bananas");
    }

    #[test]
    fn smart_case() {
        let files = vec![PathBuf::from("kitchen-sink.md")];
        let root = Path::new("tests/fixtures");
        assert!(!search(root, &files, "kitchen sink").0.is_empty());
        assert!(search(root, &files, "KITCHEN SINK").0.is_empty());
    }

    #[test]
    fn long_lines_are_cut_around_the_match() {
        let line: Vec<char> = format!("  {}needle{}", "a".repeat(500), "b".repeat(500)).chars().collect();
        let h = snippet(0, 1, 0, &line, 502, 508);
        assert!(h.text.starts_with('…'));
        assert!(h.text.chars().count() <= MAX_TEXT_CHARS + 1);
        let found: String = h.text.chars().skip(h.start).take(h.end - h.start).collect();
        assert_eq!(found, "needle");
    }
}
