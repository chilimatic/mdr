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
    let mut out = Vec::new();
    for (li, line) in doc.lines.iter().enumerate() {
        out.extend(occurrences(&line.plain, query).into_iter().map(|(start, end)| Match { line: li, start, end }));
    }
    out
}

/// Char ranges of the non-overlapping occurrences of `query` in `text`, with smart case.
pub fn occurrences(text: &str, query: &str) -> Vec<(usize, usize)> {
    let case_sensitive = query.chars().any(char::is_uppercase);
    let norm = |c: char| if case_sensitive { c } else { c.to_lowercase().next().unwrap_or(c) };
    let q: Vec<char> = query.chars().map(norm).collect();
    let chars: Vec<char> = text.chars().map(norm).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while !q.is_empty() && i + q.len() <= chars.len() {
        if chars[i..i + q.len()] == q[..] {
            out.push((i, i + q.len()));
            i += q.len();
        } else {
            i += 1;
        }
    }
    out
}
