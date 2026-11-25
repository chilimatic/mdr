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
    let key_rows = if app.key_bar { 1 } else { 0 };
    let [body, status, keys] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(key_rows)]).areas(area);
    app.on_resize(area.width, body.height);
    if app.key_bar {
        draw_keys(f, app, keys);
    }

    if app.mode == Mode::Picker {
        let list = draw_preview(f, app, body);
        draw_picker(f, app, list, status);
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

/// The keys that do something right now, as (key, action).
fn key_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let mut k = Vec::new();
    match app.mode {
        Mode::SearchInput => k.extend([("⏎", "done"), ("Esc", "cancel")]),
        Mode::Toc => k.extend([("j/k", "move"), ("⏎", "jump"), ("Esc", "close")]),
        Mode::Help => k.push(("any key", "close")),
        Mode::Picker => match app.picker.as_ref() {
            Some(pk) => match &pk.grep {
                Some(g) if g.editing => k.extend([("⏎", "search"), ("Esc", "cancel")]),
                Some(g) => {
                    if !g.hits.is_empty() {
                        k.extend([("j/k", "move"), ("⏎", "open")]);
                    }
                    k.extend([("s", "new search"), ("Esc", "file list"), ("p", "preview"), ("q", "quit")]);
                }
                None if pk.filtering => k.extend([("⏎", "keep filter"), ("Esc", "clear filter")]),
                None => {
                    if !pk.visible().is_empty() {
                        k.extend([("j/k", "move"), ("⏎", "open")]);
                    }
                    k.extend([("/", "filter names"), ("s", "search text"), ("p", "preview")]);
                    if !pk.filter.is_empty() {
                        k.push(("Esc", "clear filter"));
                    }
                    k.push(("q", "quit"));
                }
            },
            None => k.push(("q", "quit")),
        },
        Mode::Normal => {
            let searching = app.search.as_ref().is_some_and(|s| !s.matches.is_empty());
            if app.link_selected.is_some() {
                k.extend([("⏎", "open link"), ("Tab", "next link"), ("Esc", "clear")]);
            } else if searching {
                k.extend([("n/N", "match"), ("Esc", "clear")]);
            }
            if app.max_scroll() > 0 {
                k.push(("j/k", "scroll"));
            }
            if app.max_hscroll() > 0 {
                k.push(("h/l", "sideways"));
            }
            k.push(("/", "search"));
            if !app.doc.headings.is_empty() {
                k.push(("t", "contents"));
            }
            if !app.doc.links.is_empty() && app.link_selected.is_none() {
                k.push(("Tab", "links"));
            }
            if !app.jumps.is_empty() || !app.history.is_empty() {
                k.push(("⌫", "back"));
            } else if app.picker.is_some() {
                k.push(("⌫", "file list"));
            }
            let (top, bottom) = (app.scroll, app.scroll + app.view_height);
            if app.doc.code_blocks.iter().any(|c| c.first < bottom && c.last >= top) {
                k.push(("y", "copy code"));
            }
            if app.path.is_some() {
                k.push(("e", "edit"));
            }
            if !app.folded.is_empty() {
                k.push(("Z", "unfold all"));
            } else if !app.doc.headings.is_empty() {
                k.push(("z", "fold"));
            }
            // Least important: left out first when the row is full.
            if app.max_scroll() > 0 {
                k.push(("Space/b", "page"));
            }
            k.extend([("?", "all keys"), ("q", "quit")]);
        }
    }
    k
}

/// One row with the keys that work now, most important first. Hints that do not fit
/// are left out, but `? all keys` and `q quit` are always shown.
fn draw_keys(f: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let hints = key_hints(app);
    let piece = |(key, what): (&str, &str)| -> [Span<'static>; 2] {
        [Span::styled(format!(" {key} "), theme.list_marker.add_modifier(Modifier::BOLD)), Span::styled(format!("{what}  "), theme.dim)]
    };
    let width = |p: &[Span]| p.iter().map(|s| s.width()).sum::<usize>();
    let tail = hints.iter().rev().take_while(|(k, _)| matches!(*k, "?" | "q")).count();
    let (head, end) = hints.split_at(hints.len() - tail);
    let mut end: Vec<Span> = end.iter().flat_map(|&h| piece(h)).collect();
    if width(&end) > area.width as usize {
        // Very narrow: only the keys.
        end = hints[hints.len() - tail..].iter().map(|(k, _)| Span::styled(format!(" {k} "), theme.list_marker)).collect();
    }
    let mut room = (area.width as usize).saturating_sub(width(&end));
    let mut spans = Vec::new();
    for &h in head {
        let p = piece(h);
        if width(&p) > room {
            break;
        }
        room -= width(&p);
        spans.extend(p);
    }
    spans.extend(end);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
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
    if let Some(g) = &pk.grep {
        draw_grep(f, app, pk, g, body, status);
        return;
    }
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
        ])
    };
    let line = match &app.message {
        Some((msg, _)) => Line::from(vec![Span::styled(" files ", theme.status_accent), Span::styled(format!(" {msg}"), theme.status)]),
        None => line,
    };
    f.render_widget(Paragraph::new(line).style(theme.status), status);
}

/// Draws the preview of the selected file on the right, if there is room.
/// Returns the area left for the list.
fn draw_preview(f: &mut Frame, app: &mut App, body: Rect) -> Rect {
    if !app.preview_on || body.width < 90 {
        return body;
    }
    let Some(pk) = &app.picker else { return body };
    // The file, and for a search hit: where to scroll and what to highlight.
    let (path, focus) = match &pk.grep {
        Some(g) if !g.editing || !g.hits.is_empty() => match g.hits.get(g.selected) {
            Some(h) => (pk.root.join(&pk.files[h.file]), Some((h.offset, if g.editing { g.previous.clone() } else { g.query.clone() }))),
            None => return body,
        },
        Some(_) => return body,
        None => match pk.selected_path() {
            Some(p) => (p, None),
            None => return body,
        },
    };
    let list_w = (body.width * 2 / 5).clamp(30, 60);
    let [list, right] = Layout::horizontal([Constraint::Length(list_w), Constraint::Min(1)]).areas(body);
    let inner_w = right.width.saturating_sub(4) as usize;
    app.update_preview(&path, inner_w.max(10));
    let Some(pv) = &app.preview else { return body };
    let theme = &app.theme;
    let height = right.height.saturating_sub(2) as usize;
    // For a hit: the first line of its block (all lines of a block share `src`) that shows the text.
    let top = match &focus {
        Some((offset, q)) => {
            let lines = &pv.doc.lines;
            let block = lines.iter().rposition(|l| l.src <= *offset).map_or(0, |i| lines[i].src);
            let first = lines.iter().position(|l| l.src == block).unwrap_or(0);
            let hit = (first..lines.len()).find(|&i| !crate::search::occurrences(&lines[i].plain, q).is_empty()).unwrap_or(first);
            hit.saturating_sub(2)
        }
        None => 0,
    };
    let lines: Vec<Line> = pv
        .doc
        .lines
        .iter()
        .skip(top)
        .take(height)
        .map(|l| match &focus {
            Some((_, q)) => {
                let ranges: Vec<_> = crate::search::occurrences(&l.plain, q).into_iter().map(|(a, b)| (a, b, theme.search_match)).collect();
                Line::from(overlay(&l.spans, &ranges))
            }
            None => Line::from(l.spans.clone()),
        })
        .collect();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let block = Block::bordered().title(format!(" {name} ")).border_style(theme.popup_border);
    f.render_widget(Paragraph::new(lines).block(block.padding(ratatui::widgets::Padding::horizontal(1))), right);
    list
}

fn draw_grep(f: &mut Frame, app: &App, pk: &crate::picker::Picker, g: &crate::grep::Grep, body: Rect, status: Rect) {
    let theme = &app.theme;
    let items: Vec<ListItem> = if g.hits.is_empty() {
        let msg = if g.editing { "  type the text to find, ⏎ to search" } else { "  no matches" };
        vec![ListItem::new(Span::styled(msg, theme.dim))]
    } else {
        g.hits
            .iter()
            .map(|h| {
                let name = pk.files[h.file].to_string_lossy();
                let before: String = h.text.chars().take(h.start).collect();
                let found: String = h.text.chars().skip(h.start).take(h.end - h.start).collect();
                let after: String = h.text.chars().skip(h.end).collect();
                ListItem::new(Line::from(vec![
                    Span::styled(name.into_owned(), theme.list_marker),
                    Span::styled(format!(":{}  ", h.line), theme.dim),
                    Span::styled(before, theme.text),
                    Span::styled(found, theme.search_match),
                    Span::styled(after, theme.text),
                ]))
            })
            .collect()
    };
    let title = format!(" search in {} ", pk.root.display());
    let list = List::new(items)
        .block(Block::bordered().title(title).border_style(theme.popup_border))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    let mut state = ListState::default().with_selected((!g.hits.is_empty()).then_some(g.selected));
    f.render_stateful_widget(list, body, &mut state);

    let line = if let Some((msg, _)) = &app.message {
        Line::from(vec![Span::styled(" search ", theme.status_accent), Span::styled(format!(" {msg}"), theme.status)])
    } else if g.editing {
        Line::from(vec![
            Span::styled(" search ", theme.status_accent),
            Span::styled(format!(" {}█", g.query), theme.status),
        ])
    } else {
        let more = if g.truncated { " (stopped at the first matches)" } else { "" };
        Line::from(vec![
            Span::styled(" search ", theme.status_accent),
            Span::styled(
                format!(" {}  {} lines in {} files{more}", g.query, g.hits.len(), g.files_hit()),
                theme.status,
            ),
        ])
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
    ("⏎ / o", "open link (also footnotes)"),
    ("Backspace", "back: jump, file, file list"),
    ("y", "copy code block (again: next)"),
    ("z / Z", "fold section / all"),
    ("T", "toggle dark/light theme"),
    ("+/-/0", "wider/narrower/reset width"),
    ("w", "toggle watch mode"),
    ("r", "reload file"),
    ("e", "edit in $EDITOR at this line"),
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

    /// The status bar: the row above the key bar.
    fn status(s: &[String]) -> String {
        s[s.len() - 2].clone()
    }

    fn keys(s: &[String]) -> String {
        s[s.len() - 1].clone()
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
        assert!(status(&s).contains("/bananas  1/1"));

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
        assert!(status(&s).contains("light"));

        // Width change relayouts.
        key(&mut app, KeyCode::Char('-'));
        key(&mut app, KeyCode::Char('-'));
        let s = draw(&mut app, "narrower");
        assert!(status(&s).contains("width: 60"), "{:?}", status(&s));

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
        assert!(warnings[0].contains("journey"));
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
        assert!(status(&s).contains("⇆ col 1"));
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
        assert!(!status(&screen(&term)).contains('⇆'));
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
        assert!(status(&s).contains("3 of 3"));

        // Filter to one file and open it.
        key(&mut app, KeyCode::Char('/'));
        for c in "oth".chars() {
            key(&mut app, KeyCode::Char(c));
        }
        key(&mut app, KeyCode::Enter);
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(status(&screen(&term)).contains("1 of 3"));
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

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            key(app, KeyCode::Char(c));
        }
    }

    #[test]
    fn folder_search_opens_file_at_match() {
        let picker = crate::picker::Picker::scan(std::path::Path::new("tests/fixtures")).unwrap();
        let mut app = App::new(None, String::new(), ThemeKind::Dark, None, Images::new(None));
        app.start_picker(picker, false);
        let mut term = Terminal::new(TestBackend::new(80, 15)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();

        key(&mut app, KeyCode::Char('s'));
        type_text(&mut app, "bananas");
        key(&mut app, KeyCode::Enter);
        term.draw(|f| draw(f, &mut app)).unwrap();
        let s = screen(&term);
        assert!(s.iter().any(|l| l.contains("▶ kitchen-sink.md:") && l.contains("bananas")), "{s:#?}");
        assert!(status(&s).contains("1 lines in 1 files"), "{s:#?}");

        // Enter opens the file with the match selected and visible.
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.file_name(), "kitchen-sink.md");
        let m = app.search.as_ref().map(|s| s.matches[s.current]).expect("search set");
        assert!(m.line >= app.scroll && m.line < app.scroll + app.view_height);
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(status(&screen(&term)).contains("/bananas  1/1"));

        // Backspace returns to the results.
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.mode, Mode::Picker);
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(screen(&term).iter().any(|l| l.contains("search in")));

        // A new search that is cancelled goes back to the shown results.
        key(&mut app, KeyCode::Char('s'));
        type_text(&mut app, "xyz");
        key(&mut app, KeyCode::Esc);
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(status(&screen(&term)).contains("bananas  1 lines"), "{:?}", status(&screen(&term)));

        // Esc goes to the file list, with the file selected.
        key(&mut app, KeyCode::Esc);
        term.draw(|f| draw(f, &mut app)).unwrap();
        let s = screen(&term);
        assert!(status(&s).contains("3 of 3"));
        assert!(s.iter().any(|l| l.contains("▶ kitchen-sink.md")), "{s:#?}");

        // No matches.
        key(&mut app, KeyCode::Char('s'));
        type_text(&mut app, "zzzqqq");
        key(&mut app, KeyCode::Enter);
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(screen(&term).iter().any(|l| l.contains("no matches")));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.mode, Mode::Picker);
    }

    #[test]
    fn folder_search_selects_the_hit_inside_a_block() {
        let dir = std::env::temp_dir().join(format!("mdr-grep-block-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let md = "# T\n\n```text\none needle\ntwo needle\nthree\n```\n\nA later needle.\n";
        std::fs::write(dir.join("a.md"), md).unwrap();
        let picker = crate::picker::Picker::scan(&dir).unwrap();
        let mut app = App::new(None, String::new(), ThemeKind::Dark, None, Images::new(None));
        app.start_picker(picker, false);
        let mut term = Terminal::new(TestBackend::new(70, 20)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        key(&mut app, KeyCode::Char('s'));
        type_text(&mut app, "needle");
        key(&mut app, KeyCode::Enter);
        for (n, want) in [(0, "one"), (1, "two"), (2, "later")] {
            if n > 0 {
                key(&mut app, KeyCode::Backspace);
                key(&mut app, KeyCode::Char('j'));
            }
            key(&mut app, KeyCode::Enter);
            let s = app.search.as_ref().unwrap();
            let line = &app.doc.lines[s.matches[s.current].line].plain;
            assert!(line.contains(want), "hit {n}: {line:?}");
            // The editor opens at the same source line.
            app.scroll = s.matches[s.current].line;
            assert!(md.lines().nth(app.top_source_line() - 1).unwrap().contains(want));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reading_position_is_remembered() {
        let dir = std::env::temp_dir().join(format!("mdr-ui-positions-{}", std::process::id()));
        let picker = crate::picker::Picker::scan(std::path::Path::new("tests/fixtures")).unwrap();
        let mut app = App::new(None, String::new(), ThemeKind::Dark, None, Images::new(None));
        app.positions = Some(crate::positions::Positions::new(dir.join("positions")));
        app.start_picker(picker, false);
        let mut term = Terminal::new(TestBackend::new(70, 15)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        key(&mut app, KeyCode::Char('j'));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.file_name(), "kitchen-sink.md");
        term.draw(|f| draw(f, &mut app)).unwrap();
        for _ in 0..12 {
            key(&mut app, KeyCode::Char('j'));
        }
        // Inside the table, so several lines share one source offset.
        let mut n = 0;
        while app.scroll == 0 || app.doc.lines[app.scroll - 1].src != app.doc.lines[app.scroll].src {
            key(&mut app, KeyCode::Char('j'));
            n += 1;
            assert!(n < 100, "no multi-line block found");
        }
        let top = app.scroll;
        key(&mut app, KeyCode::Backspace);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.scroll, top);
        assert!(app.message.as_ref().unwrap().0.contains("where you stopped"));

        // At the top the position is forgotten.
        key(&mut app, KeyCode::Char('g'));
        key(&mut app, KeyCode::Backspace);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.scroll, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn edit_key_asks_for_the_top_source_line() {
        let mut app = app_for("tests/fixtures/kitchen-sink.md");
        let mut term = Terminal::new(TestBackend::new(70, 20)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        key(&mut app, KeyCode::Char(']'));
        key(&mut app, KeyCode::Char(']'));
        let title = app.doc.headings[app.current_heading().unwrap()].title.clone();
        key(&mut app, KeyCode::Char('e'));
        let (path, line) = app.edit_request.take().expect("edit requested");
        assert_eq!(path, std::path::Path::new("tests/fixtures/kitchen-sink.md"));
        let src = std::fs::read_to_string(&path).unwrap();
        let text = src.lines().nth(line - 1).unwrap();
        assert!(text.starts_with('#') && text.contains(&title), "line {line}: {text:?} vs {title:?}");

        let mut stdin = App::new(None, "x".into(), ThemeKind::Dark, None, Images::new(None));
        key(&mut stdin, KeyCode::Char('e'));
        assert!(stdin.edit_request.is_none());
    }

    #[test]
    fn key_bar_shows_the_keys_that_work_now() {
        let mut app = app_for("tests/fixtures/kitchen-sink.md");
        let mut term = Terminal::new(TestBackend::new(120, 20)).unwrap();
        let mut bar = |app: &mut App| {
            term.draw(|f| draw(f, app)).unwrap();
            keys(&screen(&term))
        };
        let b = bar(&mut app);
        for k in ["j/k scroll", "/ search", "t contents", "Tab links", "e edit", "? all keys", "q quit"] {
            assert!(b.contains(k), "missing {k}: {b}");
        }
        assert!(!b.contains("h/l") && !b.contains("n/N") && !b.contains("⌫"), "{b}");

        key(&mut app, KeyCode::Tab);
        assert!(bar(&mut app).contains("⏎ open link"));
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char('/'));
        let b = bar(&mut app);
        assert!(b.contains("⏎ done") && b.contains("Esc cancel") && !b.contains("q quit"), "{b}");
        type_text(&mut app, "bananas");
        key(&mut app, KeyCode::Enter);
        assert!(bar(&mut app).contains("n/N match"));
        key(&mut app, KeyCode::Char('t'));
        assert!(bar(&mut app).contains("⏎ jump"));

        // Narrow window: hints are dropped, "? all keys" and "q quit" stay.
        let mut app = app_for("tests/fixtures/kitchen-sink.md");
        let mut term = Terminal::new(TestBackend::new(40, 20)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        let b = keys(&screen(&term));
        assert!(b.contains("? all keys") && b.contains("q quit"), "{b}");
    }

    #[test]
    fn footnote_jump_and_back() {
        let md = format!("Start[^n] here.\n\n{}\n[^n]: The note text.\n", "filler\n\n".repeat(40));
        let mut app = App::new(None, md, ThemeKind::Dark, None, Images::new(None));
        let mut term = Terminal::new(TestBackend::new(60, 12)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.doc.links[app.link_selected.unwrap()].url, "#fn:n");
        key(&mut app, KeyCode::Enter);
        assert!(app.scroll > 0);
        let note = app.doc.anchors[0].1;
        assert!(note >= app.scroll && note < app.scroll + app.view_height);
        assert!(app.doc.lines[note].plain.contains("The note text"));
        assert!(key_hints(&app).contains(&("⌫", "back")));
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.scroll, 0);

        // From the footnote back to where it is used.
        key(&mut app, KeyCode::Char('G'));
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.doc.links[app.link_selected.unwrap()].url, "#fnref:n");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn copy_code_blocks_one_after_another() {
        let mut app = App::new(None, "```sh\necho one\n```\n\n```\ntwo\n```\n".into(), ThemeKind::Dark, None, Images::new(None));
        let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(keys(&screen(&term)).contains("y copy code"));
        key(&mut app, KeyCode::Char('y'));
        assert_eq!(app.clipboard_request.take().as_deref(), Some("echo one"));
        assert!(app.message.as_ref().unwrap().0.contains("copied sh (1 lines)"));
        key(&mut app, KeyCode::Char('y'));
        assert_eq!(app.clipboard_request.take().as_deref(), Some("two"));
        key(&mut app, KeyCode::Char('y'));
        assert_eq!(app.clipboard_request.take().as_deref(), Some("echo one"));

        let mut plain = App::new(None, "no code".into(), ThemeKind::Dark, None, Images::new(None));
        term.draw(|f| draw(f, &mut plain)).unwrap();
        key(&mut plain, KeyCode::Char('y'));
        assert!(plain.clipboard_request.is_none());
    }

    #[test]
    fn fold_and_unfold_sections() {
        let mut app = app_for("tests/fixtures/kitchen-sink.md");
        let mut term = Terminal::new(TestBackend::new(70, 30)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        let total = app.doc.lines.len();
        key(&mut app, KeyCode::Char(']'));
        let title = app.doc.headings[app.current_heading().unwrap()].title.clone();
        key(&mut app, KeyCode::Char('z'));
        assert!(app.doc.lines.len() < total);
        let h = &app.doc.headings[app.current_heading().unwrap()];
        assert_eq!(h.title, title);
        assert!(app.doc.lines[h.line].plain.contains("lines folded"));
        key(&mut app, KeyCode::Char('z'));
        assert_eq!(app.doc.lines.len(), total);

        key(&mut app, KeyCode::Char('Z'));
        assert!(app.doc.lines.len() < total / 2, "{} of {total}", app.doc.lines.len());
        assert!(key_hints(&app).contains(&("Z", "unfold all")));
        // Folds survive a width change, and Z opens everything.
        key(&mut app, KeyCode::Char('-'));
        assert!(app.doc.lines.len() < total / 2);
        key(&mut app, KeyCode::Char('Z'));
        assert!(app.folded.is_empty());
    }

    #[test]
    fn file_list_shows_a_preview() {
        let picker = crate::picker::Picker::scan(std::path::Path::new("tests/fixtures")).unwrap();
        let mut app = App::new(None, String::new(), ThemeKind::Dark, None, Images::new(None));
        app.start_picker(picker, false);
        let mut term = Terminal::new(TestBackend::new(120, 20)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        let s = screen(&term);
        assert!(s.iter().any(|l| l.contains("▶ diagrams.md") && l.contains(" diagrams.md ")), "{s:#?}");
        assert!(s.iter().any(|l| l.contains("Mermaid flowchart")), "{s:#?}");
        key(&mut app, KeyCode::Char('j'));
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(screen(&term).iter().any(|l| l.contains("Kitchen Sink")));

        // A search hit scrolls the preview to it.
        key(&mut app, KeyCode::Char('s'));
        type_text(&mut app, "bananas");
        key(&mut app, KeyCode::Enter);
        term.draw(|f| draw(f, &mut app)).unwrap();
        let s = screen(&term);
        // The list row, the status bar and the preview.
        assert!(s.iter().filter(|l| l.contains("bananas")).count() >= 3, "{s:#?}");

        // p hides it; narrow windows have none.
        key(&mut app, KeyCode::Char('p'));
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert_eq!(screen(&term).iter().filter(|l| l.contains("bananas")).count(), 2);
        key(&mut app, KeyCode::Char('p'));
        let mut small = Terminal::new(TestBackend::new(80, 20)).unwrap();
        small.draw(|f| draw(f, &mut app)).unwrap();
        assert_eq!(screen(&small).iter().filter(|l| l.contains("bananas")).count(), 2);
    }

    #[test]
    fn preview_of_a_large_multibyte_file_does_not_panic() {
        let dir = std::env::temp_dir().join(format!("mdr-preview-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 'é' is two bytes; byte 65536 falls inside one. No newline at all.
        std::fs::write(dir.join("big.md"), "xé".repeat(40_000)).unwrap();
        let mut app = App::new(None, String::new(), ThemeKind::Dark, None, Images::new(None));
        app.update_preview(&dir.join("big.md"), 60);
        assert!(!app.preview.as_ref().unwrap().doc.lines.is_empty());
        // A change on disk is noticed.
        std::fs::write(dir.join("big.md"), "# Changed now\n").unwrap();
        app.update_preview(&dir.join("big.md"), 60);
        assert!(app.preview.as_ref().unwrap().doc.lines[0].plain.contains("Changed now"));
        std::fs::remove_dir_all(&dir).unwrap();
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
