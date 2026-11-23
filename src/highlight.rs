//! Syntax highlighting for fenced code blocks, based on syntect with the bat syntax set.

use std::cell::OnceCell;

use ratatui::style::{Color, Modifier, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::FontStyle;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use two_face::theme::{EmbeddedLazyThemeSet, EmbeddedThemeName};

/// One highlighted line: a list of (text, style) pieces.
pub type HlLine = Vec<(String, Style)>;

/// Loads syntaxes and themes on first use, because loading takes some milliseconds.
pub struct Highlighter {
    syntaxes: OnceCell<SyntaxSet>,
    themes: OnceCell<EmbeddedLazyThemeSet>,
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl Highlighter {
    pub fn new() -> Self {
        Highlighter { syntaxes: OnceCell::new(), themes: OnceCell::new() }
    }

    fn themes(&self) -> &EmbeddedLazyThemeSet {
        self.themes.get_or_init(two_face::theme::extra)
    }

    /// Background color of the syntax theme. Code blocks use it as their fill.
    pub fn background(&self, theme: EmbeddedThemeName) -> Option<Color> {
        self.themes()
            .get(theme)
            .settings
            .background
            .map(|c| Color::Rgb(c.r, c.g, c.b))
    }

    /// Highlights `code` for the language `lang`. Returns `None` if the language is not known.
    pub fn highlight(&self, code: &str, lang: &str, theme: EmbeddedThemeName) -> Option<Vec<HlLine>> {
        if lang.is_empty() {
            return None;
        }
        let ss = self.syntaxes.get_or_init(two_face::syntax::extra_newlines);
        let syntax = ss
            .find_syntax_by_token(lang)
            .or_else(|| ss.find_syntax_by_extension(lang))?;
        let mut h = HighlightLines::new(syntax, self.themes().get(theme));
        let mut out = Vec::new();
        for line in LinesWithEndings::from(code) {
            let ranges = h.highlight_line(line, ss).ok()?;
            let pieces = ranges
                .into_iter()
                .filter_map(|(st, text)| {
                    let text = text.trim_end_matches(['\n', '\r']);
                    if text.is_empty() {
                        return None;
                    }
                    let mut style = Style::default().fg(Color::Rgb(st.foreground.r, st.foreground.g, st.foreground.b));
                    if st.font_style.contains(FontStyle::BOLD) {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if st.font_style.contains(FontStyle::ITALIC) {
                        style = style.add_modifier(Modifier::ITALIC);
                    }
                    if st.font_style.contains(FontStyle::UNDERLINE) {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    Some((text.to_string(), style))
                })
                .collect();
            out.push(pieces);
        }
        Some(out)
    }
}
