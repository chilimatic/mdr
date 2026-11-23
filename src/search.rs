//! Text search over the rendered lines.

use crate::layout::Doc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub line: usize,
    /// Char range in `Line::plain`.
    pub start: usize,
    pub end: usize,
}

pub struct Search {
    pub query: String,
    pub matches: Vec<Match>,
    pub current: usize,
}

/// Smart case: the search ignores case unless the query has an uppercase letter.
pub fn find(doc: &Doc, query: &str) -> Vec<Match> {
    if query.is_empty() {
        return Vec::new();
    }
    let case_sensitive = query.chars().any(char::is_uppercase);
    let norm = |c: char| if case_sensitive { c } else { c.to_lowercase().next().unwrap_or(c) };
    let q: Vec<char> = query.chars().map(norm).collect();
    let mut out = Vec::new();
    for (li, line) in doc.lines.iter().enumerate() {
        let chars: Vec<char> = line.plain.chars().map(norm).collect();
        let mut i = 0;
        while i + q.len() <= chars.len() {
            if chars[i..i + q.len()] == q[..] {
                out.push(Match { line: li, start: i, end: i + q.len() });
                i += q.len();
            } else {
                i += 1;
            }
        }
    }
    out
}
