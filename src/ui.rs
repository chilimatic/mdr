//! Drawing: document viewport, images, status bar and popups.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, Mode};

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let [body, status] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    app.on_resize(area.width, body.height);

    if app.mode == Mode::Picker {
        draw_picker(f, app, body, status);
        return;
    }
    let w = app.content_width().min(body.width);
    let content = Rect { x: body.x + (body.width - w) / 2, width: w, ..body };
    draw_doc(f, app, content);
    draw_images(f, app, content);
    draw_status(f, app, status);

    match app.mode {
        Mode::Toc => draw_toc(f, app, body),
        Mode::Help => draw_help(f, app, body),
        _ => {}
    }
}

/// Re-styles the char ranges `[start, end)` of a line, splitting spans where needed.
fn overlay(spans: &[Span<'static>], ranges: &[(usize, usize, Style)]) -> Vec<Span<'static>> {
    if ranges.is_empty() {
        return spans.to_vec();
    }
    let style_at = |pos: usize| ranges.iter().rev().find(|(s, e, _)| pos >= *s && pos < *e).map(|r| r.2);
    let mut out = Vec::new();
    let mut pos = 0;
    for span in spans {
        let mut buf = String::new();
        let mut cur: Option<Option<Style>> = None;
        for c in span.content.chars() {
            let st = style_at(pos);
            if cur.is_some_and(|x| x != st) {
                let s = cur.unwrap().map_or(span.style, |o| span.style.patch(o));
                out.push(Span::styled(std::mem::take(&mut buf), s));
            }
            cur = Some(st);
            buf.push(c);
            pos += 1;
        }
        if !buf.is_empty() {
            let s = cur.flatten().map_or(span.style, |o| span.style.patch(o));
            out.push(Span::styled(buf, s));
        }
    }
    out
}

fn draw_doc(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let end = (app.scroll + area.height as usize).min(app.doc.lines.len());
    let mut lines = Vec::with_capacity(area.height as usize);
    for li in app.scroll..end {
        let line = &app.doc.lines[li];
        let mut ranges = Vec::new();
        if let Some(s) = &app.search {
            for (i, m) in s.matches.iter().enumerate().filter(|(_, m)| m.line == li) {
                let style = if i == s.current { theme.search_current } else { theme.search_match };
                ranges.push((m.start, m.end, style));
            }
        }
        if let Some(sel) = app.link_selected
            && let Some(link) = app.doc.links.get(sel)
        {
            for l in app.doc.links.iter().filter(|l| l.line == li && l.id == link.id) {
                ranges.push((l.start, l.end, theme.link_selected));
            }
        }
        lines.push(Line::from(overlay(&line.spans, &ranges)));
    }
    f.render_widget(Paragraph::new(lines).scroll((0, app.hscroll.min(u16::MAX as usize) as u16)), area);
}

fn draw_images(f: &mut Frame, app: &mut App, area: Rect) {
    let top = app.scroll as i64;
    let bottom = top + area.height as i64;
    let visible: Vec<_> = app
        .doc
        .images
        .iter()
        .filter(|img| (img.line as i64) < bottom && img.line as i64 + img.rows as i64 > top)
        .map(|img| (img.path.clone(), img.line, img.x, img.cols, img.rows))
        .collect();
    for (path, line, x, cols, rows) in visible {
        // Images are not cut sideways: skip one whose left edge is scrolled out of view.
        let Some(x) = (x as usize).checked_sub(app.hscroll) else { continue };
        if let Some(proto) = app.images.protocol(&path, cols, rows) {
            let pos = SignedPosition { x: x as i16, y: (line as i64 - top) as i16 };
            f.render_widget(SlicedImage::new(proto, pos), area);
        }
    }
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    if app.mode == Mode::SearchInput {
        let count = app.search.as_ref().map(|s| s.matches.len()).unwrap_or(0);
        let line = Line::from(vec![
            Span::styled(" / ", theme.status_accent),
            Span::styled(format!(" {}", app.search_input), theme.status),
            Span::styled("█", theme.status),
            Span::styled(format!("  {count} matches "), theme.status.add_modifier(Modifier::DIM)),
        ]);
        f.render_widget(Paragraph::new(line).style(theme.status), area);
        return;
    }

    let total = app.doc.lines.len();
    let pct = if app.max_scroll() == 0 { 100 } else { app.scroll * 100 / app.max_scroll() };
    let mut right = Vec::new();
    if app.watcher.is_some() {
        right.push(Span::styled(" ● watch ", theme.status_accent));
    }
    if app.max_hscroll() > 0 {
        right.push(Span::styled(format!(" ⇆ col {} ", app.hscroll + 1), theme.status));
    }
    right.push(Span::styled(
        format!(" {}  {}/{}  {pct:>3}% ", app.theme.kind.name(), (app.scroll + 1).min(total.max(1)), total),
        theme.status,
    ));
    let right_w: usize = right.iter().map(|s| s.width()).sum();

    let middle = if let Some((msg, _)) = &app.message {
        msg.clone()
    } else if let Some(link) = app.link_selected.and_then(|i| app.doc.links.get(i)) {
        format!("→ {}", link.url)
    } else if let Some(s) = &app.search {
        if s.matches.is_empty() {
            format!("/{}  no matches", s.query)
        } else {
            format!("/{}  {}/{}", s.query, s.current + 1, s.matches.len())
        }
    } else if let Some(h) = app.current_heading().and_then(|i| app.doc.headings.get(i)) {
        h.title.clone()
    } else {
        String::new()
    };

    let name = format!(" {} ", app.file_name());
    let left_w = name.width() + 1;
    let room = (area.width as usize).saturating_sub(left_w + right_w + 1);
    let mut used = 0;
    let middle: String = middle
        .chars()
        .take_while(|c| {
            used += c.width().unwrap_or(0);
            used <= room
        })
        .collect();
    let pad = room.saturating_sub(middle.width());

    let mut spans = vec![
        Span::styled(name, theme.status_accent),
        Span::styled(" ", theme.status),
        Span::styled(middle, theme.status),
        Span::styled(" ".repeat(pad + 1), theme.status),
    ];
    spans.extend(right);
    f.render_widget(Paragraph::new(Line::from(spans)).style(theme.status), area);
}

fn draw_picker(f: &mut Frame, app: &App, body: Rect, status: Rect) {
    let theme = &app.theme;
    let Some(pk) = &app.picker else { return };
    let visible = pk.visible();
    let items: Vec<ListItem> = if visible.is_empty() {
        vec![ListItem::new(Span::styled("  no matching files", theme.dim))]
    } else {
        visible
            .iter()
            .map(|&i| {
                let p = &pk.files[i];
                let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).filter(|d| !d.is_empty());
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let mut spans = Vec::new();
                if let Some(d) = dir {
                    spans.push(Span::styled(format!("{d}/"), theme.dim));
                }
                spans.push(Span::styled(name, theme.text));
                ListItem::new(Line::from(spans))
            })
            .collect()
    };
    let title = format!(" {} ", pk.root.display());
    let list = List::new(items)
        .block(Block::bordered().title(title).border_style(theme.popup_border))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    let mut state = ListState::default().with_selected((!visible.is_empty()).then_some(pk.selected));
    f.render_stateful_widget(list, body, &mut state);

    let line = if pk.filtering {
        Line::from(vec![
            Span::styled(" / ", theme.status_accent),
            Span::styled(format!(" {}█", pk.filter), theme.status),
            Span::styled(format!("  {} of {} files", visible.len(), pk.files.len()), theme.status),
        ])
    } else {
        let filter = if pk.filter.is_empty() { String::new() } else { format!("  filter: {}", pk.filter) };
        Line::from(vec![
            Span::styled(" files ", theme.status_accent),
            Span::styled(format!(" {} of {}{filter}", visible.len(), pk.files.len()), theme.status),
            Span::styled("   ⏎ open · / filter · q quit", theme.status),
        ])
    };
    let line = match &app.message {
        Some((msg, _)) => Line::from(vec![Span::styled(" files ", theme.status_accent), Span::styled(format!(" {msg}"), theme.status)]),
        None => line,
    };
    f.render_widget(Paragraph::new(line).style(theme.status), status);
}

fn draw_toc(f: &mut Frame, app: &App, body: Rect) {
    let width = (body.width / 2).clamp(20, 50).min(body.width);
    let area = Rect { width, ..body };
    f.render_widget(Clear, area);
    let items: Vec<ListItem> = app
        .doc
        .headings
        .iter()
        .map(|h| {
            let indent = "  ".repeat(h.level.saturating_sub(1) as usize);
            let style = app.theme.headings[h.level as usize - 1];
            ListItem::new(Line::from(vec![Span::raw(indent), Span::styled(h.title.clone(), style)]))
        })
        .collect();
    let list = List::new(items)
        .block(
            Block::bordered()
                .title(" Contents ")
                .title_bottom(" ⏎ jump · esc close ")
                .border_style(app.theme.popup_border),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    let mut state = ListState::default().with_selected(Some(app.toc_selected));
    f.render_stateful_widget(list, area, &mut state);
}

const HELP: &[(&str, &str)] = &[
    ("j/k ↓/↑", "scroll line"),
    ("d/u", "half page"),
    ("space/b", "page down/up"),
    ("g/G", "top/bottom"),
    ("h/l ←/→", "scroll sideways"),
    ("]/[", "next/prev heading"),
    ("t", "table of contents"),
    ("/", "search"),
    ("n/N", "next/prev match"),
    ("Tab/S-Tab", "select link"),
    ("⏎ / o", "open link"),
    ("Backspace", "back (previous file or file list)"),
    ("T", "toggle dark/light theme"),
    ("+/-/0", "wider/narrower/reset width"),
    ("w", "toggle watch mode"),
    ("r", "reload file"),
    ("esc", "clear search/selection"),
    ("q", "quit"),
];

fn draw_help(f: &mut Frame, app: &App, body: Rect) {
    let w = 46.min(body.width);
    let h = (HELP.len() as u16 + 2).min(body.height);
    let area = Rect { x: body.x + (body.width - w) / 2, y: body.y + (body.height - h) / 2, width: w, height: h };
    f.render_widget(Clear, area);
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::styled(format!(" {k:<12}"), app.theme.list_marker),
                Span::raw(*d),
            ])
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Keys ").border_style(app.theme.popup_border)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::Images;
    use crate::theme::ThemeKind;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn screen(term: &Terminal<TestBackend>) -> Vec<String> {
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect()
    }

    fn app_for(path: &str) -> App {
        let text = std::fs::read_to_string(path).unwrap();
        let picker = ratatui_image::picker::Picker::halfblocks();
        App::new(Some(path.into()), text, ThemeKind::Dark, None, Images::new(Some(picker)))
    }

    fn key(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn end_to_end_kitchen_sink() {
        let mut app = app_for("tests/fixtures/kitchen-sink.md");
        let mut term = Terminal::new(TestBackend::new(70, 30)).unwrap();
        let mut draw = |app: &mut App, label: &str| {
            term.draw(|f| draw(f, app)).unwrap();
            let s = screen(&term);
            if std::env::var("MDR_DUMP").is_ok() {
                println!("===== {label} =====\n{}", s.join("\n"));
            }
            s
        };
        let s = draw(&mut app, "top");
        assert!(s.iter().any(|l| l.contains("Kitchen Sink")));
        assert!(!s.iter().any(|l| l.contains("front matter")));

        // Search moves the view to the match.
        key(&mut app, KeyCode::Char('/'));
        for c in "bananas".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        key(&mut app, KeyCode::Enter);
        let s = draw(&mut app, "search bananas");
        assert!(s.iter().any(|l| l.contains("bananas")));
        assert!(s.last().unwrap().contains("/bananas  1/1"));

        // Table of contents.
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char('g'));
        key(&mut app, KeyCode::Char('t'));
        let s = draw(&mut app, "toc");
        assert!(s.iter().any(|l| l.contains("Contents")));
        key(&mut app, KeyCode::Char('G'));
        key(&mut app, KeyCode::Enter);
        let s = draw(&mut app, "after toc jump to last heading");
        assert!(s.iter().any(|l| l.contains("Level four")), "{s:#?}");
        assert_eq!(app.scroll, app.max_scroll());

        // Image area and theme toggle.
        key(&mut app, KeyCode::Char('g'));
        let img_line = app.doc.images.first().expect("image laid out").line;
        app.scroll = img_line.saturating_sub(2);
        draw(&mut app, "image");
        key(&mut app, KeyCode::Char('T'));
        let s = draw(&mut app, "light theme");
        assert!(s.last().unwrap().contains("light"));

        // Width change relayouts.
        key(&mut app, KeyCode::Char('-'));
        key(&mut app, KeyCode::Char('-'));
        let s = draw(&mut app, "narrower");
        assert!(s.last().unwrap().contains("width: 60"), "{:?}", s.last());

        // Help popup.
        key(&mut app, KeyCode::Char('?'));
        let s = draw(&mut app, "help");
        assert!(s.iter().any(|l| l.contains("Keys")));
    }

    #[test]
    fn diagrams_fixture_renders() {
        let mut app = app_for("tests/fixtures/diagrams.md");
        let mut term = Terminal::new(TestBackend::new(90, 40)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        let warnings: Vec<&str> = app.doc.lines.iter().map(|l| l.plain.as_str()).filter(|l| l.contains('⚠')).collect();
        assert_eq!(warnings.len(), 1, "{warnings:#?}");
        assert!(warnings[0].contains("gantt"));
        if std::env::var("MDR_DUMP").is_ok() {
            for l in &app.doc.lines {
                println!("{}", l.plain.trim_end());
            }
        }
    }

    #[test]
    fn wide_diagram_scrolls_sideways() {
        let label = "a long node label";
        let md = format!("```mermaid\ngraph LR\nA[{label} 1] --> B[{label} 2] --> C[{label} 3] --> D[{label} 4]\n```\n");
        let mut app = App::new(None, md, ThemeKind::Dark, None, Images::new(None));
        let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        let s = screen(&term);
        assert!(s.iter().any(|l| l.contains("scroll with h / l")), "{s:#?}");
        assert!(s.iter().any(|l| l.contains("long node label 1")));
        assert!(s.last().unwrap().contains("⇆ col 1"));
        assert!(app.max_hscroll() > 0);

        for _ in 0..20 {
            key(&mut app, KeyCode::Char('l'));
        }
        assert_eq!(app.hscroll, app.max_hscroll());
        term.draw(|f| draw(f, &mut app)).unwrap();
        let s = screen(&term);
        assert!(s.iter().any(|l| l.contains("long node label 4")), "{s:#?}");
        assert!(!s.iter().any(|l| l.contains("long node label 1")));

        key(&mut app, KeyCode::Left);
        assert_eq!(app.hscroll, app.max_hscroll() - 8);

        // Search moves the view sideways to the match.
        key(&mut app, KeyCode::Char('/'));
        for c in "label 1".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        key(&mut app, KeyCode::Enter);
        term.draw(|f| draw(f, &mut app)).unwrap();
        let s = screen(&term);
        assert!(s.iter().any(|l| l.contains("label 1")), "{s:#?}");
    }

    #[test]
    fn narrow_document_does_not_scroll_sideways() {
        let mut app = App::new(None, "just text".into(), ThemeKind::Dark, None, Images::new(None));
        let mut term = Terminal::new(TestBackend::new(60, 10)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        key(&mut app, KeyCode::Char('l'));
        assert_eq!(app.hscroll, 0);
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(!screen(&term).last().unwrap().contains('⇆'));
    }

    #[test]
    fn folder_picker_opens_files_and_goes_back() {
        let picker = crate::picker::Picker::scan(std::path::Path::new("tests/fixtures")).unwrap();
        let mut app = App::new(None, String::new(), ThemeKind::Dark, None, Images::new(None));
        app.start_picker(picker, false);
        let mut term = Terminal::new(TestBackend::new(70, 15)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        let s = screen(&term);
        assert!(s.iter().any(|l| l.contains("▶ diagrams.md")), "{s:#?}");
        assert!(s.last().unwrap().contains("3 of 3"));

        // Filter to one file and open it.
        key(&mut app, KeyCode::Char('/'));
        for c in "oth".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        key(&mut app, KeyCode::Enter);
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(screen(&term).last().unwrap().contains("1 of 3"));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.file_name(), "other.md");

        // Follow a link, then Backspace twice: previous file, then the list.
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.file_name(), "kitchen-sink.md");
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.file_name(), "other.md");
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.mode, Mode::Picker);
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(screen(&term).iter().any(|l| l.contains("▶ other.md")));
    }

    #[test]
    fn readme_renders_cleanly() {
        let mut app = app_for("README.md");
        let mut term = Terminal::new(TestBackend::new(100, 40)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(!app.doc.lines.iter().any(|l| l.plain.contains('⚠')));
        // Escaped pipes in table cells render as plain pipes.
        assert!(app.doc.lines.iter().any(|l| l.plain.contains("--theme auto|dark|light")));
        assert!(app.doc.headings.iter().any(|h| h.slug == "diagrams"));
    }

    #[test]
    fn link_selection_survives_relayout() {
        let mut app = app_for("tests/fixtures/kitchen-sink.md");
        let mut term = Terminal::new(TestBackend::new(70, 30)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.doc.links[app.link_selected.unwrap()].url, "other.md");
        key(&mut app, KeyCode::Char('-'));
        key(&mut app, KeyCode::Char('-'));
        key(&mut app, KeyCode::Char('-'));
        assert_eq!(app.doc.links[app.link_selected.unwrap()].url, "other.md");
    }

    #[test]
    fn links_navigate_between_files() {
        let mut app = app_for("tests/fixtures/kitchen-sink.md");
        let mut term = Terminal::new(TestBackend::new(70, 30)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        // Select the "the table section" anchor link: second link in the document.
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.doc.links[app.link_selected.unwrap()].url, "#tables");
        key(&mut app, KeyCode::Enter);
        let tables = app.doc.headings.iter().find(|h| h.slug == "tables").unwrap().line;
        assert_eq!(app.scroll, tables.min(app.max_scroll()));

        key(&mut app, KeyCode::Char('g'));
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.doc.links[app.link_selected.unwrap()].url, "other.md");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.file_name(), "other.md");
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.file_name(), "kitchen-sink.md");
    }

    #[test]
    fn overlay_splits_spans() {
        let spans = vec![Span::raw("hello "), Span::raw("world")];
        let hi = Style::default().add_modifier(Modifier::BOLD);
        let out = overlay(&spans, &[(4, 8, hi)]);
        let texts: Vec<&str> = out.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, vec!["hell", "o ", "wo", "rld"]);
        assert_eq!(out[1].style, hi);
        assert_eq!(out[0].style, Style::default());
    }
}
