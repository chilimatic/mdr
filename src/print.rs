//! Non-interactive mode: render once and write the lines to stdout.

use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;
use ratatui::style::{Color, Modifier, Style};

use crate::highlight::Highlighter;
use crate::images::Images;
use crate::layout::{self, Ctx, Doc};
use crate::theme::Theme;

pub fn render(text: &str, path: Option<&Path>, theme: &Theme, width: usize) -> Doc {
    let hl = Highlighter::new();
    // No image protocol: images show as their alt text.
    let mut sizer = Images::new(None);
    layout::render(
        text,
        Ctx { width, theme, hl: &hl, base_dir: path.and_then(Path::parent), sizer: &mut sizer, interactive: false, link_targets: true },
    )
}

/// Writes the document. A closed pipe (for example `mdr x.md | head`) is not an error.
pub fn write(doc: &Doc, color: bool, out: &mut impl Write) -> Result<()> {
    match write_lines(doc, color, out) {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        r => Ok(r?),
    }
}

fn write_lines(doc: &Doc, color: bool, out: &mut impl Write) -> io::Result<()> {
    for line in &doc.lines {
        if color {
            let mut current = Style::default();
            for span in &line.spans {
                if span.style != current {
                    out.write_all(sgr(span.style).as_bytes())?;
                    current = span.style;
                }
                // Layout already cleans the text; this keeps the output safe on its own.
                out.write_all(crate::text::clean_line(&span.content).as_bytes())?;
            }
            if current != Style::default() {
                out.write_all(b"\x1b[0m")?;
            }
        } else {
            out.write_all(crate::text::clean_line(line.plain.trim_end()).as_bytes())?;
        }
        out.write_all(b"\n")?;
    }
    out.flush()
}

/// ANSI escape sequence for a style. It always starts with a reset, so styles never leak.
fn sgr(style: Style) -> String {
    let mut codes: Vec<String> = vec!["0".into()];
    let m = style.add_modifier;
    for (flag, code) in [
        (Modifier::BOLD, "1"),
        (Modifier::DIM, "2"),
        (Modifier::ITALIC, "3"),
        (Modifier::UNDERLINED, "4"),
        (Modifier::REVERSED, "7"),
        (Modifier::CROSSED_OUT, "9"),
    ] {
        if m.contains(flag) {
            codes.push(code.into());
        }
    }
    if let Some(c) = style.fg.and_then(|c| color(c, false)) {
        codes.push(c);
    }
    if let Some(c) = style.bg.and_then(|c| color(c, true)) {
        codes.push(c);
    }
    format!("\x1b[{}m", codes.join(";"))
}

fn color(c: Color, bg: bool) -> Option<String> {
    let base = if bg { 40 } else { 30 };
    let named = |n: u8| Some((base + n).to_string());
    let bright = |n: u8| Some((base + 60 + n).to_string());
    match c {
        Color::Reset => None,
        Color::Black => named(0),
        Color::Red => named(1),
        Color::Green => named(2),
        Color::Yellow => named(3),
        Color::Blue => named(4),
        Color::Magenta => named(5),
        Color::Cyan => named(6),
        Color::Gray => named(7),
        Color::DarkGray => bright(0),
        Color::LightRed => bright(1),
        Color::LightGreen => bright(2),
        Color::LightYellow => bright(3),
        Color::LightBlue => bright(4),
        Color::LightMagenta => bright(5),
        Color::LightCyan => bright(6),
        Color::White => bright(7),
        Color::Indexed(i) => Some(format!("{};5;{i}", base + 8)),
        Color::Rgb(r, g, b) => Some(format!("{};2;{r};{g};{b}", base + 8)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeKind;

    fn out(md: &str, color: bool) -> String {
        let theme = Theme::new(ThemeKind::Dark);
        let doc = render(md, None, &theme, 40);
        let mut buf = Vec::new();
        write(&doc, color, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn plain_has_no_escapes_or_trailing_spaces() {
        let s = out("# Title\n\n```\ncode\n```\n", false);
        assert!(!s.contains('\x1b'));
        assert!(s.lines().all(|l| l == l.trim_end()));
        assert!(s.starts_with("Title\n"));
    }

    #[test]
    fn color_output_resets_styles() {
        let s = out("**bold** text", true);
        assert!(s.contains("\x1b[0;1m"), "{s:?}");
        assert!(s.contains("\x1b[0mtext") || s.contains("\x1b[0m text"), "{s:?}");
    }

    #[test]
    fn sgr_codes() {
        assert_eq!(sgr(Style::default().fg(Color::Rgb(1, 2, 3)).bg(Color::Red)), "\x1b[0;38;2;1;2;3;41m");
        assert_eq!(sgr(Style::default().fg(Color::Indexed(200))), "\x1b[0;38;5;200m");
    }

    #[test]
    fn broken_pipe_is_not_an_error() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let theme = Theme::new(ThemeKind::Dark);
        let doc = render("hello", None, &theme, 40);
        assert!(write(&doc, false, &mut Closed).is_ok());
    }

    #[test]
    fn wide_diagram_has_no_scroll_hint() {
        let s = out("```mermaid\ngraph LR\nA[a long label one] --> B[a long label two] --> C[three]\n```\n", false);
        assert!(!s.contains("scroll with"));
        assert!(s.contains("a long label two"));
    }
}
