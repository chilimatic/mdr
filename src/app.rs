//! Application state and key handling.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::highlight::Highlighter;
use crate::images::Images;
use crate::layout::{self, Ctx, Doc};
use crate::search::{self, Search};
use crate::theme::{Theme, ThemeKind};
use crate::ui;
use crate::watch::{self, Watcher};

/// Default upper limit for the text column, for readable line lengths.
pub const DEFAULT_MAX_WIDTH: u16 = 100;
const MIN_LAYOUT_WIDTH: u16 = 20;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    SearchInput,
    Toc,
    Help,
}

pub struct App {
    pub path: Option<PathBuf>,
    pub text: String,
    pub doc: Doc,
    pub scroll: usize,
    pub view_height: usize,
    pub term_width: u16,
    pub theme: Theme,
    pub width_override: Option<u16>,
    pub hl: Highlighter,
    pub images: Images,
    pub mode: Mode,
    pub search: Option<Search>,
    pub search_input: String,
    pub toc_selected: usize,
    pub link_selected: Option<usize>,
    pub message: Option<(String, Instant)>,
    pub watcher: Option<Watcher>,
    pub history: Vec<(PathBuf, usize)>,
    layout_width: usize,
    quit: bool,
}

impl App {
    pub fn new(
        path: Option<PathBuf>,
        text: String,
        theme: ThemeKind,
        width_override: Option<u16>,
        images: Images,
    ) -> Self {
        App {
            path,
            text,
            doc: Doc::default(),
            scroll: 0,
            view_height: 0,
            term_width: 0,
            theme: Theme::new(theme),
            width_override,
            hl: Highlighter::new(),
            images,
            mode: Mode::Normal,
            search: None,
            search_input: String::new(),
            toc_selected: 0,
            link_selected: None,
            message: None,
            watcher: None,
            history: Vec::new(),
            layout_width: 0,
            quit: false,
        }
    }

    pub fn file_name(&self) -> String {
        match &self.path {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => "<stdin>".into(),
        }
    }

    pub fn content_width(&self) -> u16 {
        let max = self.width_override.unwrap_or(DEFAULT_MAX_WIDTH);
        max.min(self.term_width.saturating_sub(2)).max(MIN_LAYOUT_WIDTH)
    }

    pub fn max_scroll(&self) -> usize {
        self.doc.lines.len().saturating_sub(self.view_height.max(1))
    }

    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.message = Some((msg.into(), Instant::now()));
    }

    pub fn toggle_watch(&mut self) {
        if self.watcher.is_some() {
            self.watcher = None;
            self.set_message("watch off");
            return;
        }
        let Some(path) = self.path.clone() else {
            self.set_message("cannot watch stdin");
            return;
        };
        match watch::watch(&path) {
            Ok(w) => {
                self.watcher = Some(w);
                self.set_message("watching for changes");
            }
            Err(e) => self.set_message(format!("watch failed: {e}")),
        }
    }

    // -- layout -------------------------------------------------------------

    fn render_doc(&mut self) {
        let width = self.content_width() as usize;
        let selected_id = self.link_selected.and_then(|i| self.doc.links.get(i)).map(|l| l.id);
        let base_dir = self.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        self.images.max_rows = (self.view_height as u16).saturating_sub(2).clamp(4, 40);
        self.doc = layout::render(
            &self.text,
            Ctx {
                width,
                theme: &self.theme,
                hl: &self.hl,
                base_dir: base_dir.as_deref(),
                sizer: &mut self.images,
            },
        );
        self.layout_width = width;
        // Link ids depend only on the source, so the same link is found again after a relayout.
        self.link_selected = selected_id.and_then(|id| self.doc.links.iter().position(|l| l.id == id));
        self.rerun_search();
    }

    /// Lays out again and keeps the same source position at the top of the screen.
    pub fn relayout(&mut self) {
        let anchor = self.doc.lines.get(self.scroll).map(|l| l.src);
        self.render_doc();
        if let Some(src) = anchor {
            self.scroll = self.doc.lines.iter().position(|l| l.src >= src).unwrap_or(self.scroll);
        }
        self.scroll = self.scroll.min(self.max_scroll());
    }

    /// Called on every draw: lays out again if the terminal width changed.
    pub fn on_resize(&mut self, width: u16, height: u16) {
        let changed_height = self.view_height != height as usize;
        self.term_width = width;
        self.view_height = height as usize;
        if self.content_width() as usize != self.layout_width || (changed_height && !self.doc.images.is_empty()) {
            self.relayout();
        }
        self.scroll = self.scroll.min(self.max_scroll());
    }

    /// Reads the file again. Keeps the reader at the same heading if it still exists.
    pub fn reload(&mut self) {
        let Some(path) = self.path.clone() else { return };
        let Ok(text) = std::fs::read_to_string(&path) else {
            self.set_message("reload failed: cannot read file");
            return;
        };
        let anchor = self
            .doc
            .headings
            .iter()
            .rposition(|h| h.line <= self.scroll)
            .map(|i| {
                let h = &self.doc.headings[i];
                let nth = self.doc.headings[..i].iter().filter(|o| o.slug == h.slug).count();
                (h.slug.clone(), nth, self.scroll - h.line)
            });
        self.text = text;
        self.link_selected = None;
        self.images.clear();
        self.render_doc();
        if let Some((slug, nth, offset)) = anchor
            && let Some(h) = self.doc.headings.iter().filter(|h| h.slug == slug).nth(nth)
        {
            self.scroll = h.line + offset;
        }
        self.scroll = self.scroll.min(self.max_scroll());
        self.set_message("reloaded");
    }

    fn open_file(&mut self, path: PathBuf, anchor: Option<&str>) {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.set_message(format!("cannot open {}: {e}", path.display()));
                return;
            }
        };
        if let Some(cur) = self.path.take() {
            self.history.push((cur, self.scroll));
        }
        let watching = self.watcher.is_some();
        self.watcher = None;
        self.path = Some(path);
        self.text = text;
        self.scroll = 0;
        self.link_selected = None;
        self.search = None;
        self.images.clear();
        self.render_doc();
        if watching {
            self.toggle_watch();
        }
        if let Some(a) = anchor {
            self.jump_to_anchor(a);
        }
    }

    fn go_back(&mut self) {
        let Some((path, scroll)) = self.history.pop() else {
            self.set_message("no previous file");
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            self.set_message("cannot read previous file");
            return;
        };
        let watching = self.watcher.is_some();
        self.watcher = None;
        self.path = Some(path);
        self.text = text;
        self.link_selected = None;
        self.search = None;
        self.images.clear();
        self.render_doc();
        self.scroll = scroll.min(self.max_scroll());
        if watching {
            self.toggle_watch();
        }
    }

    // -- navigation ---------------------------------------------------------

    fn scroll_by(&mut self, delta: isize) {
        let s = self.scroll as isize + delta;
        self.scroll = (s.max(0) as usize).min(self.max_scroll());
    }

    /// Scrolls so that `line` is visible, with some context above it.
    fn reveal(&mut self, line: usize) {
        let h = self.view_height.max(1);
        if line < self.scroll || line >= self.scroll + h {
            self.scroll = line.saturating_sub(h / 3).min(self.max_scroll());
        }
    }

    fn jump_heading(&mut self, forward: bool) {
        let target = if forward {
            self.doc.headings.iter().find(|h| h.line > self.scroll)
        } else {
            self.doc.headings.iter().rev().find(|h| h.line < self.scroll)
        };
        if let Some(h) = target {
            self.scroll = h.line.min(self.max_scroll());
        }
    }

    fn jump_to_anchor(&mut self, anchor: &str) {
        let slug = layout::slugify(&percent_decode(anchor));
        if let Some(h) = self.doc.headings.iter().find(|h| h.slug == slug) {
            self.scroll = h.line.min(self.max_scroll());
        } else {
            self.set_message(format!("no heading #{anchor}"));
        }
    }

    /// Heading index of the section shown at the top of the screen.
    pub fn current_heading(&self) -> Option<usize> {
        self.doc.headings.iter().rposition(|h| h.line <= self.scroll)
    }

    fn cycle_link(&mut self, forward: bool) {
        let links = &self.doc.links;
        if links.is_empty() {
            self.set_message("no links");
            return;
        }
        let top = self.scroll;
        let bottom = self.scroll + self.view_height;
        // Index of the first piece of each link, in document order.
        let mut seen = std::collections::HashSet::new();
        let firsts: Vec<usize> = (0..links.len()).filter(|&i| seen.insert(links[i].id)).collect();
        let n = firsts.len();
        let current = self.link_selected.and_then(|sel| firsts.iter().position(|&f| links[f].id == links[sel].id));
        let visible = |k: usize| {
            let id = links[firsts[k]].id;
            links.iter().any(|l| l.id == id && l.line >= top && l.line < bottom)
        };
        let next = match current {
            Some(k) if visible(k) => {
                if forward { (k + 1) % n } else { (k + n - 1) % n }
            }
            _ => {
                if forward {
                    (0..n).find(|&k| links[firsts[k]].line >= top).unwrap_or(0)
                } else {
                    (0..n).rev().find(|&k| links[firsts[k]].line < bottom).unwrap_or(n - 1)
                }
            }
        };
        let piece = firsts[next];
        self.link_selected = Some(piece);
        self.reveal(self.doc.links[piece].line);
    }

    fn open_selected_link(&mut self) {
        let Some(url) = self.link_selected.and_then(|i| self.doc.links.get(i)).map(|l| l.url.clone()) else {
            self.set_message("no link selected (Tab selects links)");
            return;
        };
        if let Some(anchor) = url.strip_prefix('#') {
            self.jump_to_anchor(anchor);
            return;
        }
        if url.contains("://") || url.starts_with("mailto:") {
            self.spawn_open(&url);
            return;
        }
        let (file, anchor) = match url.split_once('#') {
            Some((f, a)) => (f, Some(a)),
            None => (url.as_str(), None),
        };
        let base = self.path.as_deref().and_then(Path::parent).unwrap_or(Path::new("."));
        let mut target = base.join(file);
        if target.is_relative() && !target.starts_with(".") {
            // Keeps a link like `-n` from being read as an option by the opener.
            target = Path::new(".").join(target);
        }
        let is_md = target
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"));
        if is_md {
            self.open_file(target, anchor);
        } else {
            self.spawn_open(&target.to_string_lossy());
        }
    }

    fn spawn_open(&mut self, target: &str) {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        match Command::new(opener)
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
                self.set_message(format!("opened {target}"));
            }
            Err(e) => self.set_message(format!("{opener} failed: {e}")),
        }
    }

    // -- search -------------------------------------------------------------

    fn rerun_search(&mut self) {
        if let Some(s) = &mut self.search {
            s.matches = search::find(&self.doc, &s.query);
            s.current = s.current.min(s.matches.len().saturating_sub(1));
        }
    }

    fn update_search(&mut self) {
        let matches = search::find(&self.doc, &self.search_input);
        let current = matches.iter().position(|m| m.line >= self.scroll).unwrap_or(0);
        self.search = Some(Search { query: self.search_input.clone(), matches, current });
        self.reveal_match();
    }

    fn reveal_match(&mut self) {
        if let Some(m) = self.search.as_ref().and_then(|s| s.matches.get(s.current)).copied() {
            self.reveal(m.line);
        }
    }

    fn step_match(&mut self, forward: bool) {
        let Some(s) = &self.search else {
            self.set_message("no search (press /)");
            return;
        };
        let n = s.matches.len();
        if n == 0 {
            let msg = format!("no matches for \"{}\"", s.query);
            self.set_message(msg);
            return;
        }
        let s = self.search.as_mut().unwrap();
        s.current = if forward { (s.current + 1) % n } else { (s.current + n - 1) % n };
        self.reveal_match();
    }

    // -- input --------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match self.mode {
            Mode::SearchInput => self.key_search(key),
            Mode::Toc => self.key_toc(key),
            Mode::Help => self.mode = Mode::Normal,
            Mode::Normal => self.key_normal(key),
        }
    }

    fn key_search(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.search = None;
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                if self.search_input.is_empty() {
                    self.search = None;
                } else if self.search.as_ref().is_some_and(|s| s.matches.is_empty()) {
                    self.set_message(format!("no matches for \"{}\"", self.search_input));
                }
            }
            KeyCode::Backspace => {
                self.search_input.pop();
                self.update_search();
            }
            KeyCode::Char(c) => {
                self.search_input.push(c);
                self.update_search();
            }
            _ => {}
        }
    }

    fn key_toc(&mut self, key: KeyEvent) {
        let n = self.doc.headings.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('t') | KeyCode::Char('q') => self.mode = Mode::Normal,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => self.toc_selected = (self.toc_selected + 1).min(n - 1),
            KeyCode::Up | KeyCode::Char('k') => self.toc_selected = self.toc_selected.saturating_sub(1),
            KeyCode::Home | KeyCode::Char('g') => self.toc_selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.toc_selected = n.saturating_sub(1),
            KeyCode::Enter => {
                if let Some(h) = self.doc.headings.get(self.toc_selected) {
                    self.scroll = h.line.min(self.max_scroll());
                }
                self.mode = Mode::Normal;
            }
            _ => {}
        }
    }

    fn key_normal(&mut self, key: KeyEvent) {
        let page = self.view_height.max(2) as isize - 1;
        let half = (self.view_height / 2).max(1) as isize;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => {
                self.search = None;
                self.link_selected = None;
            }
            KeyCode::Char('d') if ctrl => self.scroll_by(half),
            KeyCode::Char('u') if ctrl => self.scroll_by(-half),
            KeyCode::Char('f') if ctrl => self.scroll_by(page),
            KeyCode::Char('b') if ctrl => self.scroll_by(-page),
            KeyCode::Char('j') | KeyCode::Down => self.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_by(-1),
            KeyCode::Char('d') => self.scroll_by(half),
            KeyCode::Char('u') => self.scroll_by(-half),
            KeyCode::Char(' ') | KeyCode::Char('f') | KeyCode::PageDown => self.scroll_by(page),
            KeyCode::Char('b') | KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::Char('g') | KeyCode::Home => self.scroll = 0,
            KeyCode::Char('G') | KeyCode::End => self.scroll = self.max_scroll(),
            KeyCode::Char('/') => {
                self.mode = Mode::SearchInput;
                self.search_input.clear();
                self.search = None;
            }
            KeyCode::Char('n') => self.step_match(true),
            KeyCode::Char('N') => self.step_match(false),
            KeyCode::Char('t') => {
                if self.doc.headings.is_empty() {
                    self.set_message("no headings");
                } else {
                    self.toc_selected = self.current_heading().unwrap_or(0);
                    self.mode = Mode::Toc;
                }
            }
            KeyCode::Char(']') => self.jump_heading(true),
            KeyCode::Char('[') => self.jump_heading(false),
            KeyCode::Tab => self.cycle_link(true),
            KeyCode::BackTab => self.cycle_link(false),
            KeyCode::Enter | KeyCode::Char('o') => self.open_selected_link(),
            KeyCode::Backspace => self.go_back(),
            KeyCode::Char('T') => {
                self.theme = Theme::new(self.theme.kind.toggle());
                self.relayout();
                self.set_message(format!("theme: {}", self.theme.kind.name()));
            }
            KeyCode::Char('+') | KeyCode::Char('=') => self.change_width(4),
            KeyCode::Char('-') => self.change_width(-4),
            KeyCode::Char('0') => {
                self.width_override = None;
                self.relayout();
                self.set_message(format!("width: {}", self.content_width()));
            }
            KeyCode::Char('w') => self.toggle_watch(),
            KeyCode::Char('r') => self.reload(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
    }

    fn change_width(&mut self, delta: i32) {
        let cur = self.content_width() as i32;
        let max = self.term_width.saturating_sub(2).max(MIN_LAYOUT_WIDTH) as i32;
        let new = (cur + delta).clamp(MIN_LAYOUT_WIDTH as i32, max) as u16;
        self.width_override = Some(new);
        self.relayout();
        self.set_message(format!("width: {new}"));
    }

    // -- main loop ----------------------------------------------------------

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let size = terminal.size().context("cannot read terminal size")?;
        self.on_resize(size.width, size.height.saturating_sub(1));
        let mut dirty = true;
        while !self.quit {
            if self.message.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(3)) {
                self.message = None;
                dirty = true;
            }
            if dirty {
                terminal.draw(|f| ui::draw(f, self))?;
                dirty = false;
            }
            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(k) if k.kind != KeyEventKind::Release => {
                        self.on_key(k);
                        dirty = true;
                    }
                    Event::Resize(..) => dirty = true,
                    _ => {}
                }
            }
            let changed = self.watcher.as_ref().is_some_and(|w| w.rx.try_iter().count() > 0);
            if changed {
                self.reload();
                dirty = true;
            }
        }
        Ok(())
    }
}

/// Decodes `%XX` escapes, for link anchors such as `#caf%C3%A9`.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("caf%C3%A9"), "café");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%é1"), "%é1");
    }
}
