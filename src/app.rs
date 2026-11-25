//! Application state and key handling.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::clipboard;
use crate::grep::{self, Grep};
use crate::highlight::Highlighter;
use crate::images::Images;
use crate::layout::{self, Ctx, Doc};
use crate::picker::Picker;
use crate::positions::Positions;
use crate::search::{self, Search};
use crate::theme::{Theme, ThemeKind};
use crate::ui;
use crate::watch::{self, Watcher};

/// Default upper limit for the text column, for readable line lengths.
pub const DEFAULT_MAX_WIDTH: u16 = 100;
const MIN_LAYOUT_WIDTH: u16 = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    SearchInput,
    Toc,
    Help,
    /// File list of `mdr DIR`.
    Picker,
}

/// A rendered file for the preview next to the file list.
pub struct Preview {
    pub path: PathBuf,
    pub width: usize,
    pub theme: ThemeKind,
    pub doc: Doc,
}

pub struct App {
    pub path: Option<PathBuf>,
    pub text: String,
    pub doc: Doc,
    pub scroll: usize,
    /// Columns scrolled to the right.
    pub hscroll: usize,
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
    /// Set when mdr was started on a folder.
    pub picker: Option<Picker>,
    /// Start watching each file opened from the picker (`mdr --watch DIR`).
    pub auto_watch: bool,
    /// Where the last reading position of each file is stored. `None` in tests.
    pub positions: Option<Positions>,
    /// Set by `e`: file and source line to open in the editor. The main loop runs it,
    /// because the editor needs the terminal.
    pub edit_request: Option<(PathBuf, usize)>,
    /// Show a preview next to the file list (`p` turns it off and on).
    pub preview_on: bool,
    /// The last rendered preview.
    pub preview: Option<Preview>,
    /// Slugs of the folded sections.
    pub folded: std::collections::HashSet<String>,
    /// Positions before jumps inside the file (links to headings, footnotes, contents).
    /// `Backspace` goes back to them first.
    pub jumps: Vec<(usize, usize)>,
    /// Set by `y`: text for the clipboard. The main loop copies it (it writes to the terminal).
    pub clipboard_request: Option<String>,
    /// Last copied code block and when, so `y` again copies the next one.
    last_yank: Option<(usize, Instant)>,
    /// Editor command from the config file; else `$VISUAL` / `$EDITOR`.
    pub editor: Option<String>,
    /// Show the key bar.
    pub key_bar: bool,
    /// File changes before this time are ignored (the editor's own writes; mdr reloads after it).
    ignore_changes_until: Option<Instant>,
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
            hscroll: 0,
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
            picker: None,
            auto_watch: false,
            positions: None,
            edit_request: None,
            ignore_changes_until: None,
            editor: None,
            key_bar: true,
            preview_on: true,
            preview: None,
            folded: Default::default(),
            jumps: Vec::new(),
            clipboard_request: None,
            last_yank: None,
            layout_width: 0,
            quit: false,
        }
    }

    pub fn file_name(&self) -> String {
        match &self.path {
            // Inside a folder, show the path relative to it.
            Some(p) => match self.picker.as_ref().and_then(|pk| p.strip_prefix(&pk.root).ok()) {
                Some(rel) => rel.to_string_lossy().into_owned(),
                None => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            },
            None if self.picker.is_some() => String::new(),
            None => "<stdin>".into(),
        }
    }

    /// Renders `path` for the preview, unless the cached one fits.
    pub fn update_preview(&mut self, path: &Path, width: usize) {
        if self.preview.as_ref().is_some_and(|p| p.path == path && p.width == width && p.theme == self.theme.kind) {
            return;
        }
        // Large files: the start is enough for a preview.
        const MAX: usize = 64 * 1024;
        let mut text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_else(|e| format!("cannot read: {e}"));
        if text.len() > MAX {
            let cut = text[..MAX].rfind('\n').unwrap_or(MAX);
            text.truncate(cut);
        }
        let mut sizer = Images::new(None);
        let doc = layout::render(
            &text,
            Ctx { width, theme: &self.theme, hl: &self.hl, base_dir: path.parent(), sizer: &mut sizer, interactive: false },
        );
        self.preview = Some(Preview { path: path.to_path_buf(), width, theme: self.theme.kind, doc });
    }

    /// Shows the file list of a folder instead of a document.
    pub fn start_picker(&mut self, picker: Picker, watch: bool) {
        self.picker = Some(picker);
        self.auto_watch = watch;
        self.mode = Mode::Picker;
    }

    pub fn content_width(&self) -> u16 {
        let max = self.width_override.unwrap_or(DEFAULT_MAX_WIDTH);
        max.min(self.term_width.saturating_sub(2)).max(MIN_LAYOUT_WIDTH)
    }

    pub fn max_scroll(&self) -> usize {
        self.doc.lines.len().saturating_sub(self.view_height.max(1))
    }

    pub fn max_hscroll(&self) -> usize {
        self.doc.width.saturating_sub(self.content_width() as usize)
    }

    fn scroll_x(&mut self, delta: isize) {
        let x = self.hscroll as isize + delta;
        self.hscroll = (x.max(0) as usize).min(self.max_hscroll());
    }

    /// Scrolls sideways so the char range `[start, end)` of `line` is visible.
    fn reveal_cols(&mut self, line: usize, start: usize, end: usize) {
        let Some(l) = self.doc.lines.get(line) else { return };
        let col = |n: usize| -> usize { l.plain.chars().take(n).map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)).sum() };
        let (a, b) = (col(start), col(end));
        let w = self.content_width() as usize;
        if a < self.hscroll || b > self.hscroll + w {
            self.hscroll = a.saturating_sub(w / 3).min(self.max_hscroll());
        }
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
                interactive: true,
            },
        );
        layout::fold(&mut self.doc, &self.folded, self.theme.dim);
        self.layout_width = width;
        // Link ids depend only on the source, so the same link is found again after a relayout.
        self.link_selected = selected_id.and_then(|id| self.doc.links.iter().position(|l| l.id == id));
        self.rerun_search();
    }

    /// Lays out again and keeps the same source position at the top of the screen.
    pub fn relayout(&mut self) {
        let anchor = self.anchor();
        self.render_doc();
        if let Some(a) = anchor {
            self.scroll_to_anchor(a);
        }
        self.scroll = self.scroll.min(self.max_scroll());
        self.hscroll = self.hscroll.min(self.max_hscroll());
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
        self.hscroll = self.hscroll.min(self.max_hscroll());
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
        self.hscroll = self.hscroll.min(self.max_hscroll());
        self.set_message("reloaded");
    }

    /// Returns false if the file cannot be read; the current document stays.
    fn open_file(&mut self, path: PathBuf, anchor: Option<&str>) -> bool {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.set_message(format!("cannot open {}: {e}", path.display()));
                return false;
            }
        };
        self.save_position();
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
        // Folds and jumps belong to one file.
        self.folded.clear();
        self.jumps.clear();
        self.render_doc();
        if watching || self.auto_watch {
            self.toggle_watch();
        }
        if let Some(a) = anchor {
            self.jump_to_anchor(a);
            // Backspace should go to the previous file, not to the top of this one.
            self.jumps.clear();
        }
        true
    }

    /// Stores the reading position of the current file.
    pub fn save_position(&self) {
        let (Some(pos), Some(path)) = (&self.positions, &self.path) else { return };
        if self.mode == Mode::Picker {
            return;
        }
        pos.set(path, self.anchor().filter(|_| self.scroll > 0));
    }

    /// Scrolls to the stored reading position of the current file.
    pub fn resume_position(&mut self) {
        let (Some(pos), Some(path)) = (&self.positions, &self.path) else { return };
        let Some(a) = pos.get(path) else { return };
        self.scroll_to_anchor(a);
        if self.scroll > 0 {
            self.set_message("back where you stopped (g: top)");
        }
    }

    /// The line at the top of the screen as (block source offset, row in the block).
    /// It stays valid when the width changes.
    fn anchor(&self) -> Option<(usize, usize)> {
        let l = self.doc.lines.get(self.scroll)?;
        Some((l.src, self.scroll - self.block_start(self.scroll)))
    }

    fn scroll_to_anchor(&mut self, (src, row): (usize, usize)) {
        if let Some(first) = self.doc.lines.iter().position(|l| l.src >= src) {
            // Stay inside the block: it may have fewer rows at another width.
            let same = self.doc.lines[first..].iter().take_while(|l| l.src == self.doc.lines[first].src).count();
            self.scroll = (first + row.min(same.saturating_sub(1))).min(self.max_scroll());
        }
    }

    /// First rendered line of the block that `line` belongs to. All lines of a block
    /// (paragraph, code block, table) have the same `src`.
    fn block_start(&self, line: usize) -> usize {
        let src = self.doc.lines[line].src;
        self.doc.lines[..line].iter().rposition(|o| o.src != src).map_or(0, |i| i + 1)
    }

    /// Scrolls to the source byte `offset` (a line start) and searches for `query`,
    /// selecting the match there. Used when a file is opened from the folder search.
    fn goto_source(&mut self, offset: usize, query: &str) {
        self.search_input = query.to_string();
        let Some(last) = self.doc.lines.iter().rposition(|l| l.src <= offset) else { return };
        let block_src = self.doc.lines[last].src;
        let first = self.block_start(last);
        // The hit is the n-th occurrence in its block: count the ones before it in the source.
        let before = search::occurrences(self.text.get(block_src..offset).unwrap_or(""), query).len();
        let matches = search::find(&self.doc, query);
        let in_block: Vec<usize> = (0..matches.len()).filter(|&i| (first..=last).contains(&matches[i].line)).collect();
        let current = in_block
            .get(before)
            .or(in_block.last())
            .copied()
            .or_else(|| matches.iter().position(|m| m.line >= first));
        match current {
            Some(current) => {
                self.search = Some(Search { query: query.to_string(), matches, current });
                self.reveal_match();
            }
            None => {
                self.search = None;
                self.scroll = first.saturating_sub(self.view_height / 3).min(self.max_scroll());
            }
        }
    }

    /// Source line (from 1) of the text at the top of the screen. Inside a block, each
    /// rendered row counts as one source line, but never past the start of the next block.
    /// This is exact for code blocks and close for other blocks.
    pub fn top_source_line(&self) -> usize {
        let line_of = |src: usize| self.text.get(..src).map_or(0, |t| t.matches('\n').count()) + 1;
        let Some(l) = self.doc.lines.get(self.scroll) else { return 1 };
        let start = line_of(l.src);
        let row = self.scroll - self.block_start(self.scroll);
        let end = match self.doc.lines[self.scroll..].iter().find(|o| o.src > l.src) {
            Some(next) => line_of(next.src).saturating_sub(1).max(start),
            None => self.text.lines().count().max(start),
        };
        (start + row).min(end)
    }

    fn go_back(&mut self) {
        if let Some(a) = self.jumps.pop() {
            self.scroll_to_anchor(a);
            return;
        }
        self.save_position();
        let Some((path, scroll)) = self.history.pop() else {
            if let Some(pk) = self.picker.as_mut() {
                self.mode = Mode::Picker;
                self.message = None;
                // Select the file the reader comes from.
                if let Some(cur) = &self.path
                    && let Some(i) = pk.visible().iter().position(|&f| pk.root.join(&pk.files[f]) == *cur)
                {
                    pk.selected = i;
                }
            } else {
                self.set_message("no previous file");
            }
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
        // Folds and jumps belong to one file.
        self.folded.clear();
        self.jumps.clear();
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

    /// Jumps to a heading slug, a footnote (`fn:label`) or a footnote reference (`fnref:label`).
    fn jump_to_anchor(&mut self, anchor: &str) {
        let anchor = percent_decode(anchor);
        let target = if let Some(label) = anchor.strip_prefix("fnref:") {
            let url = format!("#fn:{label}");
            self.doc.links.iter().find(|l| l.url == url).map(|l| l.line)
        } else if let Some(a) = self.doc.anchors.iter().find(|(name, _)| *name == anchor) {
            Some(a.1)
        } else {
            let slug = layout::slugify(&anchor);
            self.doc.headings.iter().find(|h| h.slug == slug).map(|h| h.line)
        };
        match target {
            Some(line) => self.jump_to_line(line),
            None if anchor.starts_with("fn") => self.set_message("footnote not found (folded?)"),
            None => self.set_message(format!("no heading #{anchor}")),
        }
    }

    /// Scrolls to `line` and remembers the old position for `Backspace`.
    fn jump_to_line(&mut self, line: usize) {
        if let Some(a) = self.anchor() {
            self.jumps.push(a);
            if self.jumps.len() > 100 {
                self.jumps.remove(0);
            }
        }
        self.scroll = line.min(self.max_scroll());
    }

    /// `z`: folds or unfolds the section at the top of the screen.
    fn toggle_fold(&mut self) {
        let Some(i) = self.current_heading() else {
            self.set_message("no section here (z folds the section at the top)");
            return;
        };
        let slug = self.doc.headings[i].slug.clone();
        if !self.folded.remove(&slug) {
            self.folded.insert(slug.clone());
        }
        self.relayout();
        if let Some(h) = self.doc.headings.iter().find(|h| h.slug == slug) {
            self.scroll = h.line.min(self.max_scroll());
        }
    }

    /// `Z`: unfolds everything, or folds all sections from the main level down.
    fn toggle_fold_all(&mut self) {
        if !self.folded.is_empty() {
            self.folded.clear();
            self.relayout();
            self.set_message("unfolded all");
            return;
        }
        // The main level: the highest level that has more than one heading.
        let Some(level) = (1..=6).find(|&lv| self.doc.headings.iter().filter(|h| h.level == lv).count() > 1) else {
            self.set_message("nothing to fold");
            return;
        };
        self.folded = self.doc.headings.iter().filter(|h| h.level >= level).map(|h| h.slug.clone()).collect();
        self.relayout();
        self.scroll = self.scroll.min(self.max_scroll());
        self.set_message("folded all (z opens one, Z opens all)");
    }

    /// `y`: copies the first code block on screen; `y` again soon after copies the next one.
    fn yank(&mut self) {
        let (top, bottom) = (self.scroll, self.scroll + self.view_height.max(1));
        let visible: Vec<usize> =
            (0..self.doc.code_blocks.len()).filter(|&i| self.doc.code_blocks[i].first < bottom && self.doc.code_blocks[i].last >= top).collect();
        let Some(&first) = visible.first() else {
            self.set_message("no code block on screen");
            return;
        };
        let pick = match self.last_yank {
            Some((prev, t)) if t.elapsed() < Duration::from_secs(3) => {
                visible.iter().position(|&v| v == prev).map_or(first, |p| visible[(p + 1) % visible.len()])
            }
            _ => first,
        };
        self.last_yank = Some((pick, Instant::now()));
        let c = &self.doc.code_blocks[pick];
        let lines = c.text.lines().count();
        let what = if c.lang.is_empty() { "code".to_string() } else { c.lang.clone() };
        let start: String = c.text.lines().next().unwrap_or("").trim().chars().take(30).collect();
        let more = if visible.len() > 1 { " · y: next block" } else { "" };
        let msg = format!("copied {what} ({lines} lines): {start}{more}");
        self.clipboard_request = Some(c.text.clone());
        self.set_message(msg);
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
        let l = &self.doc.links[piece];
        let (line, start, end) = (l.line, l.start, l.end);
        self.reveal(line);
        self.reveal_cols(line, start, end);
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
            self.reveal_cols(m.line, m.start, m.end);
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
            Mode::Picker => self.key_picker(key),
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

    fn key_picker(&mut self, key: KeyEvent) {
        if self.picker.as_ref().is_some_and(|pk| pk.grep.is_some()) {
            self.key_grep(key);
            return;
        }
        let Some(pk) = self.picker.as_mut() else {
            self.mode = Mode::Normal;
            return;
        };
        if pk.filtering {
            match key.code {
                KeyCode::Esc => {
                    pk.filter.clear();
                    pk.filtering = false;
                }
                KeyCode::Enter => pk.filtering = false,
                KeyCode::Backspace => {
                    pk.filter.pop();
                }
                KeyCode::Char(c) => {
                    pk.filter.push(c);
                    pk.selected = 0;
                }
                KeyCode::Down => pk.selected += 1,
                KeyCode::Up => pk.selected = pk.selected.saturating_sub(1),
                _ => {}
            }
            pk.clamp();
            return;
        }
        let page = self.view_height.max(2) - 1;
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => pk.filter.clear(),
            KeyCode::Char('/') => {
                pk.filtering = true;
                pk.filter.clear();
            }
            KeyCode::Char('j') | KeyCode::Down => pk.selected += 1,
            KeyCode::Char('k') | KeyCode::Up => pk.selected = pk.selected.saturating_sub(1),
            KeyCode::Char('g') | KeyCode::Home => pk.selected = 0,
            KeyCode::Char('G') | KeyCode::End => pk.selected = usize::MAX,
            KeyCode::PageDown | KeyCode::Char(' ') => pk.selected = pk.selected.saturating_add(page),
            KeyCode::PageUp | KeyCode::Char('b') => pk.selected = pk.selected.saturating_sub(page),
            KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('l') | KeyCode::Right => {
                if let Some(path) = pk.selected_path()
                    && self.open_from_picker(path)
                {
                    self.resume_position();
                }
                return;
            }
            KeyCode::Char('s') => {
                pk.grep = Some(Grep { editing: true, ..Grep::default() });
            }
            KeyCode::Char('p') => {
                self.preview_on = !self.preview_on;
                return;
            }
            _ => {}
        }
        pk.clamp();
    }

    /// Opens a file from the list or the folder search. A file opened there starts a new history.
    fn open_from_picker(&mut self, path: PathBuf) -> bool {
        let (old_path, old_history) = (self.path.take(), std::mem::take(&mut self.history));
        self.mode = Mode::Normal;
        if self.open_file(path, None) {
            return true;
        }
        self.path = old_path;
        self.history = old_history;
        self.mode = Mode::Picker;
        false
    }

    fn key_grep(&mut self, key: KeyEvent) {
        let page = self.view_height.max(2) - 1;
        let Some(pk) = self.picker.as_mut() else { return };
        let Some(g) = pk.grep.as_mut() else { return };
        if g.editing {
            match key.code {
                // Back to the shown results, if there are any.
                KeyCode::Esc | KeyCode::Enter if g.query.is_empty() || key.code == KeyCode::Esc => {
                    if g.previous.is_empty() {
                        pk.grep = None;
                    } else {
                        g.query = std::mem::take(&mut g.previous);
                        g.editing = false;
                    }
                }
                KeyCode::Enter => {
                    g.previous.clear();
                    let (hits, truncated) = grep::search(&pk.root, &pk.files, &g.query);
                    g.hits = hits;
                    g.truncated = truncated;
                    g.selected = 0;
                    g.editing = false;
                }
                KeyCode::Backspace => {
                    g.query.pop();
                }
                KeyCode::Char(c) => g.query.push(c),
                _ => {}
            }
            return;
        }
        let last = g.hits.len().saturating_sub(1);
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => pk.grep = None,
            KeyCode::Char('s') | KeyCode::Char('/') => {
                g.previous = std::mem::take(&mut g.query);
                g.editing = true;
            }
            KeyCode::Char('p') => self.preview_on = !self.preview_on,
            KeyCode::Char('j') | KeyCode::Down => g.selected = (g.selected + 1).min(last),
            KeyCode::Char('k') | KeyCode::Up => g.selected = g.selected.saturating_sub(1),
            KeyCode::Char('g') | KeyCode::Home => g.selected = 0,
            KeyCode::Char('G') | KeyCode::End => g.selected = last,
            KeyCode::PageDown | KeyCode::Char(' ') => g.selected = (g.selected + page).min(last),
            KeyCode::PageUp | KeyCode::Char('b') => g.selected = g.selected.saturating_sub(page),
            KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('l') | KeyCode::Right => {
                let Some(hit) = g.hits.get(g.selected) else { return };
                let (path, offset, query) = (pk.root.join(&pk.files[hit.file]), hit.offset, g.query.clone());
                if self.open_from_picker(path) {
                    self.goto_source(offset, &query);
                }
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
                self.mode = Mode::Normal;
                if let Some(line) = self.doc.headings.get(self.toc_selected).map(|h| h.line) {
                    self.jump_to_line(line);
                }
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
            KeyCode::Char('h') | KeyCode::Left => self.scroll_x(-8),
            KeyCode::Char('l') | KeyCode::Right => self.scroll_x(8),
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
            KeyCode::Char('y') => self.yank(),
            KeyCode::Char('z') => self.toggle_fold(),
            KeyCode::Char('Z') => self.toggle_fold_all(),
            KeyCode::Char('e') => match self.path.clone() {
                Some(p) => self.edit_request = Some((p, self.top_source_line())),
                None => self.set_message("cannot edit stdin"),
            },
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
        let result = self.event_loop(terminal);
        self.save_position();
        result
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let size = terminal.size().context("cannot read terminal size")?;
        // The status bar and the key bar take two rows.
        self.on_resize(size.width, size.height.saturating_sub(1 + self.key_bar as u16));
        self.resume_position();
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
                        if let Some((path, line)) = self.edit_request.take() {
                            self.run_editor(terminal, &path, line)?;
                        }
                        if let Some(text) = self.clipboard_request.take()
                            && let Err(e) = clipboard::copy(&text)
                        {
                            self.set_message(format!("copy failed: {e}"));
                        }
                        dirty = true;
                    }
                    Event::Resize(..) => dirty = true,
                    _ => {}
                }
            }
            let changed = self.watcher.as_ref().is_some_and(|w| w.rx.try_iter().count() > 0);
            let ignored = self.ignore_changes_until.is_some_and(|t| Instant::now() < t);
            if changed && !ignored {
                self.reload();
                dirty = true;
            }
        }
        Ok(())
    }

    /// Hands the terminal to `$VISUAL` / `$EDITOR` (default `vi`) and reloads afterwards.
    fn run_editor(&mut self, terminal: &mut DefaultTerminal, path: &Path, line: usize) -> Result<()> {
        use ratatui::crossterm::{cursor, execute, terminal as ct};
        let editor = self
            .editor
            .clone()
            .or_else(|| ["VISUAL", "EDITOR"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.trim().is_empty()))
            .unwrap_or_else(|| "vi".into());
        let first = editor_program(&editor);
        let program = Path::new(first).file_name().and_then(|n| n.to_str()).unwrap_or(first).to_string();
        let args = editor_args(&program, &path.to_string_lossy(), line);

        let mut out = std::io::stdout();
        ct::disable_raw_mode()?;
        execute!(out, ct::LeaveAlternateScreen, cursor::Show)?;
        // Like git: the shell reads $EDITOR, so quotes and options in it work.
        let status = Command::new("sh").arg("-c").arg(format!("{editor} \"$@\"")).arg("mdr").args(&args).status();
        ct::enable_raw_mode()?;
        execute!(out, ct::EnterAlternateScreen, ct::Clear(ct::ClearType::All))?;
        // A new terminal redraws everything. `Terminal::clear` would ask the terminal
        // for the cursor position, and not every terminal answers.
        *terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;

        // The images were on the old screen.
        self.images.clear();
        // The watcher reports the edits up to its debounce delay later; the reload below covers them.
        self.ignore_changes_until = Some(Instant::now() + Duration::from_millis(500));
        match status {
            Ok(s) if s.success() => self.reload(),
            // 127: the shell did not find the editor.
            Ok(s) if s.code() == Some(127) => self.set_message(format!("cannot start {program} (set $EDITOR)")),
            Ok(s) => {
                self.reload();
                self.set_message(format!("{program} exited with {s}"));
            }
            Err(e) => self.set_message(format!("cannot start {program}: {e} (set $EDITOR)")),
        }
        Ok(())
    }
}

/// The program in an editor command: the first word, or the quoted text at the start.
fn editor_program(cmd: &str) -> &str {
    let cmd = cmd.trim_start();
    match cmd.chars().next() {
        Some(q @ ('"' | '\'')) => cmd[1..].split(q).next().unwrap_or(""),
        _ => cmd.split_whitespace().next().unwrap_or(""),
    }
}

/// Arguments that open `path` at `line` in a known editor. Unknown editors get only the path.
fn editor_args(program: &str, path: &str, line: usize) -> Vec<String> {
    let name = Path::new(program).file_name().and_then(|n| n.to_str()).unwrap_or(program);
    match name {
        "vi" | "vim" | "nvim" | "gvim" | "mvim" | "view" | "nano" | "pico" | "emacs" | "emacsclient" | "micro"
        | "kak" | "joe" | "ne" | "mg" | "jed" | "mcedit" => vec![format!("+{line}"), path.into()],
        "hx" | "helix" | "subl" | "zed" => vec![format!("{path}:{line}")],
        "code" | "codium" | "code-insiders" | "cursor" => vec!["--goto".into(), format!("{path}:{line}")],
        _ => vec![path.into()],
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
    use super::{editor_args, editor_program, percent_decode};

    #[test]
    fn editor_program_honours_quotes() {
        assert_eq!(editor_program("nvim"), "nvim");
        assert_eq!(editor_program("  code --wait"), "code");
        assert_eq!(editor_program("\"/opt/My Editor/hx\" -c x"), "/opt/My Editor/hx");
        assert_eq!(editor_program("'/a b/vim'"), "/a b/vim");
    }

    #[test]
    fn editor_line_arguments() {
        assert_eq!(editor_args("/usr/bin/nvim", "a.md", 12), ["+12", "a.md"]);
        assert_eq!(editor_args("hx", "a.md", 3), ["a.md:3"]);
        assert_eq!(editor_args("code", "a.md", 3), ["--goto", "a.md:3"]);
        assert_eq!(editor_args("gedit", "a.md", 3), ["a.md"]);
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("caf%C3%A9"), "café");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%é1"), "%é1");
    }
}
