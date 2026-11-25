//! Turns markdown source into a list of styled terminal lines for a given width.
//!
//! The output is a structured line model (`Doc`), not ANSI text. Search, heading
//! navigation, link selection and image placement all work on this model.

use std::path::{Path, PathBuf};

use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::highlight::Highlighter;
use crate::theme::Theme;

/// Narrowest text column we lay out into, even inside deep nesting.
const MIN_WIDTH: usize = 8;
/// Deepest nesting of quotes and lists that is indented.
const MAX_NEST: usize = 12;
/// Larger code blocks, or blocks with longer lines, are shown without highlighting.
const MAX_HIGHLIGHT_BYTES: usize = 256 * 1024;
const MAX_HIGHLIGHT_LINE: usize = 4096;

pub struct Line {
    pub spans: Vec<Span<'static>>,
    /// The line text without styles. Char positions here match char positions in `spans`.
    pub plain: String,
    /// Byte offset in the source of the block this line belongs to.
    pub src: usize,
}

pub struct Heading {
    pub level: u8,
    pub title: String,
    pub slug: String,
    pub line: usize,
}

/// One visible piece of a link. A link that wraps over two lines gives two entries.
pub struct Link {
    /// Same for all pieces of one link.
    pub id: usize,
    pub url: String,
    pub line: usize,
    /// Char range in `Line::plain`.
    pub start: usize,
    pub end: usize,
}

pub struct ImageBlock {
    pub path: PathBuf,
    pub line: usize,
    pub x: u16,
    pub cols: u16,
    pub rows: u16,
}

/// A fenced or indented code block (also a diagram), for copying.
pub struct CodeBlock {
    pub lang: String,
    pub text: String,
    /// Rendered lines `first..=last`.
    pub first: usize,
    pub last: usize,
}

#[derive(Default)]
pub struct Doc {
    pub lines: Vec<Line>,
    pub headings: Vec<Heading>,
    pub links: Vec<Link>,
    pub images: Vec<ImageBlock>,
    pub code_blocks: Vec<CodeBlock>,
    /// Link targets that are not headings: `fn:label` for footnote definitions.
    pub anchors: Vec<(String, usize)>,
    /// Width of the widest line. Can be more than the layout width (wide diagrams).
    pub width: usize,
}

/// Gives the cell size of a local image, fitted into `max_cols`. `None` means "do not show it".
pub trait ImageSizer {
    fn size(&mut self, path: &Path, max_cols: u16) -> Option<(u16, u16)>;
}

#[cfg(test)]
pub struct NoImages;

#[cfg(test)]
impl ImageSizer for NoImages {
    fn size(&mut self, _: &Path, _: u16) -> Option<(u16, u16)> {
        None
    }
}

pub struct Ctx<'a> {
    pub width: usize,
    pub theme: &'a Theme,
    pub hl: &'a Highlighter,
    pub base_dir: Option<&'a Path>,
    pub sizer: &'a mut dyn ImageSizer,
    /// Interactive viewer (true) or printed output (false). Only the viewer can scroll sideways.
    pub interactive: bool,
    /// Show link targets after the link text, for printed output.
    pub link_targets: bool,
}

pub fn render(src: &str, ctx: Ctx<'_>) -> Doc {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_GFM);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_MATH);

    let mut b = Builder::new(ctx);
    for (event, range) in Parser::new_ext(src, opts).into_offset_iter() {
        b.event(event, range.start);
    }
    b.flush_inline();
    b.doc.width = b.doc.lines.iter().map(|l| l.plain.width()).max().unwrap_or(0);
    b.doc
}

/// Hides the content of the sections whose heading slug is in `folded`, and adds
/// "▸ N lines folded" to those headings. Line numbers in `doc` are updated.
pub fn fold(doc: &mut Doc, folded: &std::collections::HashSet<String>, mark: Style, width: usize) {
    if folded.is_empty() {
        return;
    }
    let n = doc.lines.len();
    let mut keep = vec![true; n];
    let mut marks = Vec::new();
    for (i, h) in doc.headings.iter().enumerate() {
        if !keep[h.line] || !folded.contains(&h.slug) {
            continue;
        }
        let src = doc.lines[h.line].src;
        let start = (h.line + 1..n).find(|&l| doc.lines[l].src != src).unwrap_or(n);
        let mut end = doc.headings[i + 1..].iter().find(|o| o.level <= h.level).map_or(n, |o| o.line);
        // Keep the blank line before the next heading.
        if end < n && end > start && doc.lines[end - 1].plain.trim().is_empty() {
            end -= 1;
        }
        if end <= start {
            continue;
        }
        keep[start..end].iter_mut().for_each(|k| *k = false);
        // The mark goes on the last row of the title, not on the underline or the blank line.
        let mut last = h.line;
        while last + 1 < start && {
            let p = doc.lines[last + 1].plain.trim();
            !p.is_empty() && !p.chars().all(|c| c == '━' || c == '─')
        } {
            last += 1;
        }
        marks.push((last, end - start));
    }
    for (line, count) in marks {
        let long = format!("  ▸ {count} lines folded");
        let used = doc.lines[line].plain.width();
        let text = if used + long.width() <= width { long } else { format!(" ▸{count}") };
        let l = &mut doc.lines[line];
        l.plain.push_str(&text);
        l.spans.push(Span::styled(text, mark));
    }
    // New index of each old line; a hidden line maps to the last line kept before it.
    let mut map = vec![0usize; n];
    let mut next = 0usize;
    for i in 0..n {
        map[i] = if keep[i] { next } else { next.saturating_sub(1) };
        if keep[i] {
            next += 1;
        }
    }
    let mut i = 0;
    doc.lines.retain(|_| {
        i += 1;
        keep[i - 1]
    });
    doc.headings.retain(|h| keep[h.line]);
    doc.headings.iter_mut().for_each(|h| h.line = map[h.line]);
    doc.links.retain(|l| keep[l.line]);
    doc.links.iter_mut().for_each(|l| l.line = map[l.line]);
    doc.images.retain(|im| keep[im.line]);
    doc.images.iter_mut().for_each(|im| im.line = map[im.line]);
    doc.code_blocks.retain(|c| keep[c.first]);
    doc.code_blocks.iter_mut().for_each(|c| {
        c.first = map[c.first];
        c.last = map[c.last];
    });
    doc.anchors.retain(|a| a.1 < n && keep[a.1]);
    doc.anchors.iter_mut().for_each(|a| a.1 = map[a.1]);
    doc.width = doc.lines.iter().map(|l| l.plain.width()).max().unwrap_or(0);
}

/// GitHub-style heading anchor: lowercase, spaces become dashes, punctuation is dropped.
pub fn slugify(s: &str) -> String {
    s.trim()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c.to_lowercase().next().unwrap_or(c))
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Inline segments and wrapping
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Seg {
    text: String,
    style: Style,
    link: Option<usize>,
    hard_break: bool,
}

impl Seg {
    fn new(text: impl Into<String>, style: Style, link: Option<usize>) -> Self {
        Seg { text: text.into(), style, link, hard_break: false }
    }
}

enum Tok {
    /// Non-space pieces that touch each other, e.g. `foo**bar**`. Never split unless too long.
    Word(Vec<Seg>),
    Space(Seg),
    Break,
}

fn tokenize(segs: &[Seg]) -> Vec<Tok> {
    let mut toks: Vec<Tok> = Vec::new();
    for seg in segs {
        if seg.hard_break {
            toks.push(Tok::Break);
            continue;
        }
        let mut buf = String::new();
        let mut buf_space = false;
        let push = |toks: &mut Vec<Tok>, buf: &mut String, space: bool| {
            if buf.is_empty() {
                return;
            }
            let piece = Seg::new(std::mem::take(buf), seg.style, seg.link);
            if space {
                toks.push(Tok::Space(piece));
            } else if let Some(Tok::Word(w)) = toks.last_mut() {
                w.push(piece);
            } else {
                toks.push(Tok::Word(vec![piece]));
            }
        };
        for c in seg.text.chars() {
            let c = if c == '\n' || c == '\t' { ' ' } else { c };
            let is_space = c == ' ';
            if is_space != buf_space {
                push(&mut toks, &mut buf, buf_space);
                buf_space = is_space;
            }
            buf.push(c);
        }
        push(&mut toks, &mut buf, buf_space);
    }
    toks
}

fn segs_width(segs: &[Seg]) -> usize {
    segs.iter().map(|s| s.text.width()).sum()
}

/// Greedy word wrap of styled segments into lines of at most `width` columns.
fn wrap(segs: &[Seg], width: usize) -> Vec<Vec<Seg>> {
    let width = width.max(1);
    let mut lines: Vec<Vec<Seg>> = vec![Vec::new()];
    let mut cur = 0usize;
    let mut space: Option<Seg> = None;
    for tok in tokenize(segs) {
        match tok {
            Tok::Break => {
                lines.push(Vec::new());
                cur = 0;
                space = None;
            }
            Tok::Space(s) => {
                if cur > 0 {
                    space = Some(Seg::new(" ", s.style, s.link));
                }
            }
            Tok::Word(word) => {
                let w = segs_width(&word);
                if cur > 0 && cur + 1 + w > width {
                    lines.push(Vec::new());
                    cur = 0;
                    space = None;
                }
                if let Some(s) = space.take() {
                    lines.last_mut().unwrap().push(s);
                    cur += 1;
                }
                if cur + w <= width {
                    lines.last_mut().unwrap().extend(word);
                    cur += w;
                    continue;
                }
                // Word longer than the line: break it by characters.
                for piece in word {
                    let mut buf = String::new();
                    for c in piece.text.chars() {
                        let cw = c.width().unwrap_or(0);
                        if cur + cw > width && cur > 0 {
                            if !buf.is_empty() {
                                lines.last_mut().unwrap().push(Seg::new(std::mem::take(&mut buf), piece.style, piece.link));
                            }
                            lines.push(Vec::new());
                            cur = 0;
                        }
                        buf.push(c);
                        cur += cw;
                    }
                    if !buf.is_empty() {
                        lines.last_mut().unwrap().push(Seg::new(buf, piece.style, piece.link));
                    }
                }
            }
        }
    }
    while lines.len() > 1 && lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

/// Breaks pre-styled pieces into lines of at most `width` columns, by characters.
fn hard_wrap(pieces: Vec<(String, Style)>, width: usize) -> Vec<Vec<(String, Style)>> {
    let width = width.max(1);
    let mut lines = vec![Vec::new()];
    let mut cur = 0;
    for (text, style) in pieces {
        let mut buf = String::new();
        for c in text.chars() {
            let cw = c.width().unwrap_or(0);
            if cur + cw > width && cur > 0 {
                if !buf.is_empty() {
                    lines.last_mut().unwrap().push((std::mem::take(&mut buf), style));
                }
                lines.push(Vec::new());
                cur = 0;
            }
            buf.push(c);
            cur += cw;
        }
        if !buf.is_empty() {
            lines.last_mut().unwrap().push((buf, style));
        }
    }
    lines
}

// ---------------------------------------------------------------------------
// Block builder
// ---------------------------------------------------------------------------

enum Container {
    Quote,
    List { next: Option<u64> },
    Item { marker: String, marker_style: Style, width: usize, pending: bool },
}

struct TableB {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Vec<Seg>>>,
    header_rows: usize,
    /// Cells that were left out because the table is too large.
    dropped: usize,
    cells: usize,
}

/// Larger tables are cut: the rest of the cells are counted, not drawn.
const MAX_TABLE_COLS: usize = 64;
const MAX_TABLE_CELLS: usize = 50_000;

struct Builder<'a> {
    ctx: Ctx<'a>,
    doc: Doc,
    containers: Vec<Container>,
    segs: Vec<Seg>,
    styles: Vec<Style>,
    link_urls: Vec<String>,
    cur_link: Option<usize>,
    need_blank: bool,
    heading: Option<u8>,
    code: Option<(String, String)>,
    table: Option<TableB>,
    image: Option<(String, String)>,
    html_block: bool,
    /// Inside `<!-- ... -->` that spans several lines of an HTML block.
    html_comment: bool,
    /// An HTML block that has shown a line (comment-only blocks show nothing).
    html_shown: bool,
    /// A `---` / `+++` block that is not at the start of the file: shown as text, with
    /// this delimiter.
    meta: Option<&'static str>,
    /// Index in `segs` where the current link text starts.
    link_start: usize,
    footnote_label: Option<String>,
    skip: bool,
    src: usize,
}

impl<'a> Builder<'a> {
    fn new(ctx: Ctx<'a>) -> Self {
        Builder {
            ctx,
            doc: Doc::default(),
            containers: Vec::new(),
            segs: Vec::new(),
            styles: Vec::new(),
            link_urls: Vec::new(),
            cur_link: None,
            need_blank: false,
            heading: None,
            code: None,
            table: None,
            image: None,
            html_block: false,
            html_comment: false,
            html_shown: false,
            meta: None,
            link_start: 0,
            footnote_label: None,
            skip: false,
            src: 0,
        }
    }

    fn theme(&self) -> &Theme {
        self.ctx.theme
    }

    fn style(&self) -> Style {
        self.styles.iter().fold(self.theme().text, |acc, s| acc.patch(*s))
    }

    fn push_text(&mut self, text: &str) {
        let style = self.style();
        self.segs.push(Seg::new(text, style, self.cur_link));
    }

    // -- prefixes -----------------------------------------------------------

    fn container_width(c: &Container) -> usize {
        match c {
            Container::Quote => 2,
            Container::List { .. } => 0,
            Container::Item { width, .. } => *width,
        }
    }

    /// How many containers get a prefix: at most `MAX_NEST`, and never so many that the
    /// text column gets narrower than `MIN_WIDTH`. Deeper nesting is not indented further,
    /// so a document with thousands of nested quotes cannot blow up the output.
    fn shown_containers(&self) -> usize {
        let room = self.ctx.width.saturating_sub(MIN_WIDTH);
        let mut used = 0;
        let mut n = 0;
        for c in self.containers.iter().take(MAX_NEST) {
            let w = Self::container_width(c);
            if used + w > room {
                break;
            }
            used += w;
            n += 1;
        }
        n
    }

    fn prefix_width(&self) -> usize {
        self.containers.iter().take(self.shown_containers()).map(Self::container_width).sum()
    }

    fn avail(&self) -> usize {
        self.ctx.width.saturating_sub(self.prefix_width()).max(MIN_WIDTH)
    }

    fn any_pending(&self) -> bool {
        self.containers.iter().any(|c| matches!(c, Container::Item { pending: true, .. }))
    }

    /// Builds the prefix for the next line. A pending list marker is used once and then cleared.
    fn take_prefix(&mut self) -> Vec<Span<'static>> {
        let bar = self.theme().quote_bar;
        let shown = self.shown_containers();
        let mut out = Vec::new();
        for (i, c) in self.containers.iter_mut().enumerate() {
            if i >= shown {
                // Not indented further, but a new item still shows its marker.
                if let Container::Item { marker, marker_style, pending, .. } = c
                    && *pending
                {
                    out.push(Span::styled(format!("{marker} "), *marker_style));
                    *pending = false;
                }
                continue;
            }
            match c {
                Container::Quote => out.push(Span::styled("│ ", bar)),
                Container::List { .. } => {}
                Container::Item { marker, marker_style, width, pending } => {
                    if *pending {
                        let pad = width.saturating_sub(marker.width());
                        out.push(Span::styled(marker.clone(), *marker_style));
                        out.push(Span::raw(" ".repeat(pad)));
                        *pending = false;
                    } else {
                        out.push(Span::raw(" ".repeat(*width)));
                    }
                }
            }
        }
        out
    }

    /// Prefix for an empty separator line: quote bars only, list markers stay pending.
    fn blank_prefix(&self) -> Vec<Span<'static>> {
        let bar = self.theme().quote_bar;
        let mut out = Vec::new();
        for c in self.containers.iter().take(self.shown_containers()) {
            match c {
                Container::Quote => out.push(Span::styled("│", bar)),
                Container::Item { width, .. } => out.push(Span::raw(" ".repeat(*width))),
                Container::List { .. } => {}
            }
        }
        // Trailing spaces only matter before a quote bar.
        while out.last().is_some_and(|s| s.content.trim().is_empty()) {
            out.pop();
        }
        out
    }

    // -- line output --------------------------------------------------------

    fn push_line(&mut self, mut spans: Vec<Span<'static>>) -> usize {
        // Document text must never carry escape sequences or bidi controls to the terminal.
        for s in &mut spans {
            if let std::borrow::Cow::Owned(clean) = crate::text::clean_line(&s.content) {
                s.content = clean.into();
            }
        }
        let plain: String = spans.iter().map(|s| s.content.as_ref()).collect();
        self.doc.lines.push(Line { spans, plain, src: self.src });
        self.doc.lines.len() - 1
    }

    fn blank_line(&mut self) {
        let p = self.blank_prefix();
        self.push_line(p);
    }

    /// Called before a block starts: ends open inline text and adds a separator line if needed.
    fn begin_block(&mut self) {
        self.flush_inline();
        if self.need_blank && !self.any_pending() && !self.doc.lines.is_empty() {
            // The separator belongs to the block above, so a block's rows start with its text.
            let prev = self.doc.lines.last().map_or(self.src, |l| l.src);
            let p = self.blank_prefix();
            let i = self.push_line(p);
            self.doc.lines[i].src = prev;
        }
        self.need_blank = false;
    }

    /// Emits wrapped segment lines with prefixes, and records link positions.
    fn emit_wrapped(&mut self, lines: Vec<Vec<Seg>>) {
        for segs in lines {
            let mut spans = self.take_prefix();
            let mut col: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            let line_idx = self.doc.lines.len();
            let mut open: Option<(usize, usize)> = None; // (link index, start col)
            for seg in segs {
                let n = seg.text.chars().count();
                match (open, seg.link) {
                    (Some((l, _)), Some(k)) if l == k => {}
                    (prev, next) => {
                        if let Some((l, start)) = prev {
                            self.doc.links.push(Link { id: l, url: self.link_urls[l].clone(), line: line_idx, start, end: col });
                        }
                        open = next.map(|k| (k, col));
                    }
                }
                col += n;
                spans.push(Span::styled(seg.text, seg.style));
            }
            if let Some((l, start)) = open {
                self.doc.links.push(Link { id: l, url: self.link_urls[l].clone(), line: line_idx, start, end: col });
            }
            self.push_line(spans);
        }
    }

    /// Writes pending inline text as wrapped lines.
    fn flush_inline(&mut self) {
        if self.segs.iter().all(|s| s.text.trim().is_empty() && !s.hard_break) {
            self.segs.clear();
            return;
        }
        let segs = std::mem::take(&mut self.segs);
        let lines = wrap(&segs, self.avail());
        self.emit_wrapped(lines);
    }

    // -- events -------------------------------------------------------------

    fn event(&mut self, ev: Event<'_>, offset: usize) {
        if self.skip {
            if let Event::End(TagEnd::MetadataBlock(_)) = ev {
                self.skip = false;
            }
            return;
        }
        if self.meta.is_some()
            && let Event::Text(t) = &ev
        {
            self.raw_lines(t, self.theme().dim);
            return;
        }
        match ev {
            Event::Start(tag) => self.start(tag, offset),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                if let Some((_, body)) = &mut self.code {
                    body.push_str(&t);
                } else if let Some((_, alt)) = &mut self.image {
                    alt.push_str(&t);
                } else {
                    self.push_text(&t);
                }
            }
            Event::Code(t) => {
                if let Some((_, alt)) = &mut self.image {
                    alt.push_str(&t);
                } else {
                    let style = self.style().patch(self.theme().code_inline);
                    self.segs.push(Seg::new(t.to_string(), style, self.cur_link));
                }
            }
            Event::InlineMath(t) => {
                let style = self.style().patch(self.theme().math);
                self.segs.push(Seg::new(crate::math::to_unicode(&t, false), style, self.cur_link));
            }
            Event::DisplayMath(t) if self.table.is_some() || self.heading.is_some() || self.cur_link.is_some() => {
                // No room for lines of its own here: keep it in the text.
                let style = self.style().patch(self.theme().math);
                self.segs.push(Seg::new(crate::math::to_unicode(&t, false), style, self.cur_link));
            }
            Event::DisplayMath(t) => {
                // A formula on its own lines, centered.
                self.flush_inline();
                let style = self.theme().math;
                let text = crate::math::to_unicode(&t, true);
                let w = self.avail();
                for l in text.lines() {
                    let pad = w.saturating_sub(l.width()) / 2;
                    let mut spans = self.take_prefix();
                    spans.push(Span::raw(" ".repeat(pad)));
                    spans.push(Span::styled(l.to_string(), style));
                    self.push_line(spans);
                }
                self.flush_inline();
            }
            Event::Html(t) => {
                if self.html_block {
                    // pulldown-cmark sends a block line by line, so a comment can span events.
                    let dim = self.theme().dim;
                    for l in t.lines() {
                        let shown = self.strip_comments(l);
                        if shown.trim().is_empty() {
                            continue;
                        }
                        if !self.html_shown {
                            self.begin_block();
                            self.html_shown = true;
                        }
                        let mut spans = self.take_prefix();
                        spans.push(Span::styled(shown, dim));
                        self.push_line(spans);
                    }
                }
            }
            Event::InlineHtml(t) => {
                let tag = t.trim().to_ascii_lowercase();
                if tag.starts_with("<br") {
                    self.segs.push(Seg { text: String::new(), style: Style::default(), link: None, hard_break: true });
                } else if !tag.starts_with("<!--") {
                    let style = self.theme().dim;
                    self.segs.push(Seg::new(t.to_string(), style, None));
                }
            }
            Event::FootnoteReference(label) => {
                // A link to the footnote; the footnote links back (see `fnref:`).
                let style = self.theme().link;
                self.link_urls.push(format!("#fn:{label}"));
                let link = Some(self.link_urls.len() - 1);
                self.segs.push(Seg::new(format!("[^{label}]"), style, link));
            }
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => {
                self.segs.push(Seg { text: String::new(), style: Style::default(), link: None, hard_break: true });
            }
            Event::Rule => {
                self.src = offset;
                self.begin_block();
                let w = self.avail();
                let mut spans = self.take_prefix();
                spans.push(Span::styled("─".repeat(w), self.theme().rule));
                self.push_line(spans);
                self.need_blank = true;
            }
            Event::TaskListMarker(done) => {
                let (mark, style) = if done {
                    ("☑", self.theme().headings[3])
                } else {
                    ("☐", self.theme().list_marker)
                };
                if let Some(Container::Item { marker, marker_style, width, .. }) = self
                    .containers
                    .iter_mut()
                    .rev()
                    .find(|c| matches!(c, Container::Item { .. }))
                {
                    *marker = mark.to_string();
                    *marker_style = style;
                    *width = 2;
                }
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>, offset: usize) {
        match tag {
            Tag::Paragraph => {
                self.src = offset;
                self.begin_block();
                self.push_footnote_label();
            }
            Tag::Heading { level, .. } => {
                self.src = offset;
                self.begin_block();
                let n = heading_num(level);
                self.heading = Some(n);
                self.styles.push(self.theme().headings[n as usize - 1]);
            }
            Tag::BlockQuote(kind) => {
                self.src = offset;
                self.begin_block();
                self.containers.push(Container::Quote);
                if let Some(kind) = kind {
                    let label = match kind {
                        BlockQuoteKind::Note => "ℹ Note",
                        BlockQuoteKind::Tip => "💡 Tip",
                        BlockQuoteKind::Important => "❗ Important",
                        BlockQuoteKind::Warning => "⚠ Warning",
                        BlockQuoteKind::Caution => "⛔ Caution",
                    };
                    let mut spans = self.take_prefix();
                    spans.push(Span::styled(label, self.theme().quote_label));
                    self.push_line(spans);
                }
                self.styles.push(self.theme().dim);
            }
            Tag::CodeBlock(kind) => {
                self.src = offset;
                self.begin_block();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split([' ', ',', '{']).next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::HtmlBlock => {
                // The block starts when it shows its first line (see `Event::Html`).
                self.flush_inline();
                self.src = offset;
                self.html_block = true;
                self.html_shown = false;
            }
            Tag::List(start) => {
                let in_item = self.containers.iter().any(|c| matches!(c, Container::Item { .. }));
                if in_item {
                    self.flush_inline();
                } else {
                    self.src = offset;
                    self.begin_block();
                }
                self.containers.push(Container::List { next: start });
            }
            Tag::Item => {
                self.src = offset;
                self.flush_inline();
                if self.need_blank && !self.any_pending() {
                    self.blank_line();
                }
                self.need_blank = false;
                let depth = self.containers.iter().filter(|c| matches!(c, Container::List { .. })).count();
                let marker = match self.containers.last_mut() {
                    Some(Container::List { next: Some(n) }) => {
                        let m = format!("{n}.");
                        *n += 1;
                        m
                    }
                    _ => ["•", "◦", "▪"][(depth.saturating_sub(1)) % 3].to_string(),
                };
                let width = marker.width() + 1;
                let marker_style = self.theme().list_marker;
                self.containers.push(Container::Item { marker, marker_style, width, pending: true });
            }
            Tag::FootnoteDefinition(label) => {
                self.src = offset;
                self.begin_block();
                self.doc.anchors.push((format!("fn:{label}"), self.doc.lines.len()));
                self.footnote_label = Some(label.to_string());
            }
            Tag::Table(aligns) => {
                self.src = offset;
                self.begin_block();
                self.table = Some(TableB { aligns, rows: Vec::new(), header_rows: 0, dropped: 0, cells: 0 });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.rows.push(Vec::new());
                }
            }
            Tag::TableCell => self.segs.clear(),
            Tag::Emphasis => self.styles.push(self.theme().emph),
            Tag::Strong => self.styles.push(self.theme().strong),
            Tag::Strikethrough => self.styles.push(self.theme().strike),
            Tag::Superscript | Tag::Subscript => self.styles.push(Style::default()),
            Tag::Link { dest_url, .. } => {
                self.link_urls.push(dest_url.to_string());
                self.cur_link = Some(self.link_urls.len() - 1);
                self.link_start = self.segs.len();
                self.styles.push(self.theme().link);
            }
            Tag::Image { dest_url, .. } => {
                self.image = Some((dest_url.to_string(), String::new()));
            }
            // Front matter is hidden only at the very start of the file.
            Tag::MetadataBlock(kind) if offset > 0 => {
                self.src = offset;
                self.begin_block();
                let delim = if matches!(kind, pulldown_cmark::MetadataBlockKind::PlusesStyle) { "+++" } else { "---" };
                self.raw_lines(delim, self.theme().dim);
                self.meta = Some(delim);
            }
            Tag::MetadataBlock(_) => self.skip = true,
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {
                self.begin_block();
            }
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_inline();
                self.need_blank = true;
            }
            TagEnd::Heading(level) => {
                self.styles.pop();
                self.heading = None;
                self.end_heading(heading_num(level));
                self.need_blank = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                self.styles.pop();
                self.containers.pop();
                self.need_blank = true;
            }
            TagEnd::CodeBlock => {
                if let Some((lang, body)) = self.code.take() {
                    self.emit_code(&lang, &body);
                }
                self.need_blank = true;
            }
            TagEnd::HtmlBlock => {
                self.html_block = false;
                self.html_comment = false;
                if self.html_shown {
                    self.need_blank = true;
                }
            }
            TagEnd::List(_) => {
                self.flush_inline();
                self.containers.pop();
                let in_item = self.containers.iter().any(|c| matches!(c, Container::Item { .. }));
                self.need_blank = !in_item;
            }
            TagEnd::Item => {
                self.flush_inline();
                if let Some(Container::Item { pending: true, .. }) = self.containers.last() {
                    let p = self.take_prefix();
                    self.push_line(p);
                }
                self.containers.pop();
            }
            TagEnd::FootnoteDefinition => {
                self.push_footnote_label();
                self.flush_inline();
                self.need_blank = true;
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.emit_table(t);
                }
                self.need_blank = true;
            }
            TagEnd::TableHead => {
                if let Some(t) = &mut self.table {
                    t.header_rows = t.rows.len();
                }
            }
            TagEnd::TableRow => {}
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.segs);
                if let Some(t) = self.table.as_mut() {
                    let fits = t.cells < MAX_TABLE_CELLS && t.rows.last().is_some_and(|r| r.len() < MAX_TABLE_COLS);
                    match t.rows.last_mut() {
                        Some(row) if fits => {
                            row.push(cell);
                            t.cells += 1;
                        }
                        _ => t.dropped += 1,
                    }
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Superscript | TagEnd::Subscript => {
                self.styles.pop();
            }
            TagEnd::Link => {
                self.styles.pop();
                // Printed text cannot be clicked: show where the link goes.
                if self.ctx.link_targets
                    && let Some(url) = self.cur_link.map(|l| self.link_urls[l].clone())
                {
                    let text: String = self.segs.get(self.link_start..).unwrap_or_default().iter().map(|s| s.text.as_str()).collect();
                    let bare = url.strip_prefix("mailto:").unwrap_or(&url);
                    if !url.is_empty() && !url.starts_with('#') && text.trim() != bare {
                        let style = self.theme().dim;
                        self.segs.push(Seg::new(format!(" ({url})"), style, None));
                    }
                }
                self.cur_link = None;
            }
            TagEnd::Image => {
                if let Some((url, alt)) = self.image.take() {
                    self.end_image(url, alt);
                }
            }
            TagEnd::MetadataBlock(_) => {
                if let Some(delim) = self.meta.take() {
                    self.raw_lines(delim, self.theme().dim);
                    self.need_blank = true;
                }
            }
            TagEnd::DefinitionList | TagEnd::DefinitionListTitle | TagEnd::DefinitionListDefinition => {
                self.flush_inline();
                self.need_blank = true;
            }
        }
    }

    // -- block emitters -----------------------------------------------------

    /// Lines of text as they are, one rendered line each.
    fn raw_lines(&mut self, text: &str, style: Style) {
        for l in text.lines() {
            let mut spans = self.take_prefix();
            spans.push(Span::styled(l.to_string(), style));
            self.push_line(spans);
        }
    }

    /// The part of an HTML line outside `<!-- ... -->`, tracking comments across lines.
    fn strip_comments(&mut self, line: &str) -> String {
        let mut out = String::new();
        let mut rest = line;
        loop {
            if self.html_comment {
                match rest.find("-->") {
                    Some(k) => {
                        self.html_comment = false;
                        rest = &rest[k + 3..];
                    }
                    None => return out,
                }
            } else {
                match rest.find("<!--") {
                    Some(k) => {
                        out.push_str(&rest[..k]);
                        self.html_comment = true;
                        rest = &rest[k + 4..];
                    }
                    None => {
                        out.push_str(rest);
                        return out;
                    }
                }
            }
        }
    }

    /// The `[^label]: ` in front of a footnote, linking back to where it is used.
    fn push_footnote_label(&mut self) {
        if let Some(label) = self.footnote_label.take() {
            self.link_urls.push(format!("#fnref:{label}"));
            let link = Some(self.link_urls.len() - 1);
            let style = self.theme().dim;
            self.segs.push(Seg::new(format!("[^{label}]"), style, link));
            self.segs.push(Seg::new(": ".to_string(), style, None));
        }
    }

    fn end_heading(&mut self, level: u8) {
        let segs = std::mem::take(&mut self.segs);
        let title: String = segs.iter().map(|s| s.text.as_str()).collect::<String>().trim().to_string();
        let style = self.theme().headings[level as usize - 1];
        let mut all = Vec::new();
        if level >= 3 {
            all.push(Seg::new(format!("{} ", "#".repeat(level as usize)), self.theme().heading_rule, None));
        }
        all.extend(segs);
        let line = self.doc.lines.len();
        let lines = wrap(&all, self.avail());
        let title_w = lines.iter().map(|l| segs_width(l)).max().unwrap_or(0);
        self.emit_wrapped(lines);
        if level <= 2 {
            let ch = if level == 1 { "━" } else { "─" };
            let rule_style = if level == 1 { style.remove_modifier(ratatui::style::Modifier::BOLD) } else { self.theme().heading_rule };
            let mut spans = self.take_prefix();
            spans.push(Span::styled(ch.repeat(title_w.max(3)), rule_style));
            self.push_line(spans);
        }
        let base = slugify(&title);
        let dupes = self.doc.headings.iter().filter(|h| h.slug == base || h.slug.strip_prefix(&base).is_some_and(|r| r.starts_with('-') && r[1..].parse::<u32>().is_ok())).count();
        let slug = if dupes == 0 { base } else { format!("{base}-{dupes}") };
        self.doc.headings.push(Heading { level, slug, title, line });
    }

    fn end_image(&mut self, url: String, alt: String) {
        let is_remote = url.contains("://") || url.starts_with("data:");
        // Image rows cannot sit inside a table cell or a heading: show the alt text there.
        let inline_only = self.table.is_some() || self.heading.is_some();
        if !is_remote && !inline_only {
            let path = match self.ctx.base_dir {
                Some(dir) => dir.join(&url),
                None => PathBuf::from(&url),
            };
            let max_cols = self.avail().min(u16::MAX as usize) as u16;
            if let Some((cols, rows)) = self.ctx.sizer.size(&path, max_cols) {
                self.flush_inline();
                let line = self.doc.lines.len();
                let x = self.prefix_width() as u16;
                for _ in 0..rows {
                    let p = self.take_prefix();
                    self.push_line(p);
                }
                self.doc.images.push(ImageBlock { path, line, x, cols, rows });
                return;
            }
        }
        let label = if alt.trim().is_empty() { url.clone() } else { alt };
        // An image inside a link (a badge) belongs to the outer link.
        let link = match self.cur_link {
            Some(l) => l,
            None => {
                self.link_urls.push(url);
                self.link_urls.len() - 1
            }
        };
        let style = self.style().patch(self.theme().image_alt);
        self.segs.push(Seg::new(format!("🖼 {label}"), style, Some(link)));
    }

    fn emit_code(&mut self, lang: &str, body: &str) {
        let first = self.doc.lines.len();
        self.emit_code_rows(lang, body);
        if self.doc.lines.len() > first {
            let text = body.strip_suffix('\n').unwrap_or(body).to_string();
            self.doc.code_blocks.push(CodeBlock { lang: lang.to_string(), text, first, last: self.doc.lines.len() - 1 });
        }
    }

    fn emit_code_rows(&mut self, lang: &str, body: &str) {
        if crate::diagram::is_diagram(lang) {
            match crate::diagram::render(lang, body, self.avail(), self.ctx.theme) {
                Ok(rows) => {
                    let widest = rows.iter().map(|r| r.iter().map(|(t, _)| t.width()).sum::<usize>()).max().unwrap_or(0);
                    if widest > self.avail() && self.ctx.interactive {
                        let mut spans = self.take_prefix();
                        spans.push(Span::styled("↔ wider than the window: scroll with h / l", self.theme().dim));
                        self.push_line(spans);
                    }
                    for row in rows {
                        let mut spans = self.take_prefix();
                        spans.extend(row.into_iter().map(|(t, st)| Span::styled(t, st)));
                        self.push_line(spans);
                    }
                    return;
                }
                Err(msg) => {
                    // Show why, then fall back to the source as a normal code block.
                    let mut spans = self.take_prefix();
                    spans.push(Span::styled(format!("⚠ {lang}: {msg}"), self.theme().dim));
                    self.push_line(spans);
                }
            }
        }
        let theme_syntax = self.theme().syntax;
        let bg = self.ctx.hl.background(theme_syntax).unwrap_or(Color::Reset);
        let body = body.replace('\t', "    ");
        let body = body.strip_suffix('\n').unwrap_or(&body);
        let highlight = body.len() <= MAX_HIGHLIGHT_BYTES && body.lines().all(|l| l.len() <= MAX_HIGHLIGHT_LINE);
        let lines: Vec<Vec<(String, Style)>> = highlight
            .then(|| self.ctx.hl.highlight(body, lang, theme_syntax))
            .flatten()
            .unwrap_or_else(|| body.lines().map(|l| vec![(l.to_string(), self.theme().text)]).collect());
        let w = self.avail();
        let inner = w.saturating_sub(4).max(1);
        let fill = Style::default().bg(bg);

        // Top padding line with the language label on the right.
        let mut spans = self.take_prefix();
        let label = if lang.is_empty() { String::new() } else { format!("{lang} ") };
        let lw = label.width().min(w);
        spans.push(Span::styled(" ".repeat(w - lw), fill));
        spans.push(Span::styled(label, self.theme().code_label.bg(bg)));
        self.push_line(spans);

        for line in lines {
            for row in hard_wrap(line, inner) {
                let mut spans = self.take_prefix();
                spans.push(Span::styled("  ", fill));
                let mut used = 0;
                for (text, style) in row {
                    used += text.width();
                    spans.push(Span::styled(text, style.bg(bg)));
                }
                spans.push(Span::styled(" ".repeat(w.saturating_sub(2 + used)), fill));
                self.push_line(spans);
            }
        }
        let mut spans = self.take_prefix();
        spans.push(Span::styled(" ".repeat(w), fill));
        self.push_line(spans);
    }

    fn emit_table(&mut self, t: TableB) {
        let ncols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if ncols == 0 {
            return;
        }
        // Column widths: natural width, shrunk from the widest column until the table fits.
        let mut widths = vec![1usize; ncols];
        for row in &t.rows {
            for (c, cell) in row.iter().enumerate() {
                widths[c] = widths[c].max(segs_width(cell));
            }
        }
        let overhead = 3 * ncols + 1;
        let avail = self.avail().saturating_sub(overhead);
        // Narrow the widest columns to a common cap: the largest cap (at least 3) that fits.
        // A binary search, so a very wide table costs O(columns × log width).
        if widths.iter().sum::<usize>() > avail {
            let fits = |cap: usize| widths.iter().map(|&w| w.min(cap)).sum::<usize>() <= avail;
            let (mut lo, mut hi) = (3, widths.iter().copied().max().unwrap_or(3));
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                if fits(mid) { lo = mid } else { hi = mid - 1 }
            }
            for w in &mut widths {
                *w = (*w).min(lo);
            }
        }

        let border = self.theme().table_border;
        let rule = |l: &str, m: &str, r: &str| -> Span<'static> {
            let parts: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
            Span::styled(format!("{l}{}{r}", parts.join(m)), border)
        };
        let top = rule("┌", "┬", "┐");
        let mid = rule("├", "┼", "┤");
        let bottom = rule("└", "┴", "┘");

        let mut spans = self.take_prefix();
        spans.push(top);
        self.push_line(spans);

        let header_style = self.theme().table_header;
        for (ri, row) in t.rows.iter().enumerate() {
            let is_header = ri < t.header_rows;
            let cells: Vec<Vec<Vec<Seg>>> = (0..ncols)
                .map(|c| {
                    let mut segs = row.get(c).cloned().unwrap_or_default();
                    if is_header {
                        for s in &mut segs {
                            s.style = s.style.patch(header_style);
                        }
                    }
                    wrap(&segs, widths[c])
                })
                .collect();
            let height = cells.iter().map(|c| c.len()).max().unwrap_or(1);
            for li in 0..height {
                let mut spans = self.take_prefix();
                let mut col: usize = spans.iter().map(|s| s.content.chars().count()).sum();
                let line_idx = self.doc.lines.len();
                spans.push(Span::styled("│", border));
                col += 1;
                for c in 0..ncols {
                    let segs = cells[c].get(li).cloned().unwrap_or_default();
                    let used = segs_width(&segs);
                    let free = widths[c].saturating_sub(used);
                    let (lpad, rpad) = match t.aligns.get(c) {
                        Some(Alignment::Right) => (free, 0),
                        Some(Alignment::Center) => (free / 2, free - free / 2),
                        _ => (0, free),
                    };
                    spans.push(Span::raw(" ".repeat(1 + lpad)));
                    col += 1 + lpad;
                    for seg in segs {
                        let n = seg.text.chars().count();
                        if let Some(l) = seg.link {
                            self.doc.links.push(Link { id: l, url: self.link_urls[l].clone(), line: line_idx, start: col, end: col + n });
                        }
                        col += n;
                        spans.push(Span::styled(seg.text, seg.style));
                    }
                    spans.push(Span::raw(" ".repeat(rpad + 1)));
                    col += rpad + 1;
                    spans.push(Span::styled("│", border));
                    col += 1;
                }
                self.push_line(spans);
            }
            if is_header && ri + 1 == t.header_rows {
                let mut spans = self.take_prefix();
                spans.push(mid.clone());
                self.push_line(spans);
            }
        }
        let mut spans = self.take_prefix();
        spans.push(bottom);
        self.push_line(spans);
        if t.dropped > 0 {
            let mut spans = self.take_prefix();
            spans.push(Span::styled(format!("⚠ table too large: {} cells not shown", t.dropped), self.theme().dim));
            self.push_line(spans);
        }
    }
}

fn heading_num(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Theme, ThemeKind};

    fn plain(md: &str, width: usize) -> Vec<String> {
        let theme = Theme::new(ThemeKind::Dark);
        let hl = Highlighter::new();
        let mut sizer = NoImages;
        let doc = render(md, Ctx { width, theme: &theme, hl: &hl, base_dir: None, sizer: &mut sizer, interactive: true, link_targets: false });
        doc.lines.into_iter().map(|l| l.plain.trim_end().to_string()).collect()
    }

    fn doc_of(md: &str, width: usize) -> Doc {
        let theme = Theme::new(ThemeKind::Dark);
        let hl = Highlighter::new();
        let mut sizer = NoImages;
        render(md, Ctx { width, theme: &theme, hl: &hl, base_dir: None, sizer: &mut sizer, interactive: true, link_targets: false })
    }

    #[test]
    fn footnotes_link_both_ways() {
        let doc = doc_of("Text[^a] here.\n\n[^a]: The note.\n", 40);
        let urls: Vec<&str> = doc.links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(urls, ["#fn:a", "#fnref:a"]);
        let (name, line) = &doc.anchors[0];
        assert_eq!(name, "fn:a");
        assert!(doc.lines[*line].plain.contains("The note."), "{:?}", doc.lines[*line].plain);
        assert!(doc.lines[*line].plain.starts_with("[^a]: "));
    }

    #[test]
    fn records_code_blocks() {
        let doc = doc_of("Intro\n\n```rust\nfn main() {}\nlet x = 1;\n```\n\nOutro\n", 40);
        assert_eq!(doc.code_blocks.len(), 1);
        let c = &doc.code_blocks[0];
        assert_eq!((c.lang.as_str(), c.text.as_str()), ("rust", "fn main() {}\nlet x = 1;"));
        assert!(doc.lines[c.first + 1].plain.contains("fn main"));
        assert!(doc.lines[c.last + 2].plain.contains("Outro"));
    }

    #[test]
    fn fold_keys_are_unique_for_repeated_headings() {
        let mut doc = doc_of("## Example\n\none\n\n## Example\n\ntwo\n\n## Example 1\n\nthree\n", 40);
        let slugs: Vec<&str> = doc.headings.iter().map(|h| h.slug.as_str()).collect();
        let unique: std::collections::HashSet<&str> = slugs.iter().copied().collect();
        assert_eq!(unique.len(), slugs.len(), "{slugs:?}");
        let folded: std::collections::HashSet<String> = [doc.headings[1].slug.clone()].into();
        fold(&mut doc, &folded, Style::default(), 40);
        let text: Vec<&str> = doc.lines.iter().map(|l| l.plain.trim_end()).collect();
        assert!(text.contains(&"one") && !text.contains(&"two") && text.contains(&"three"), "{text:#?}");
    }

    #[test]
    fn folds_sections() {
        let md = "# Top\n\nintro\n\n## A\n\na1\n\n### A.1\n\ndeep [link](x)\n\n## B\n\nb1\n";
        let mut doc = doc_of(md, 40);
        let folded: std::collections::HashSet<String> = ["a".to_string()].into();
        fold(&mut doc, &folded, Style::default(), 40);
        let text: Vec<&str> = doc.lines.iter().map(|l| l.plain.trim_end()).collect();
        assert!(text.iter().any(|l| l.starts_with("A") && l.contains("▸ 5 lines folded")), "{text:#?}");
        assert!(!text.iter().any(|l| l.contains("a1") || l.contains("deep")), "{text:#?}");
        assert!(text.contains(&"b1"));
        let titles: Vec<&str> = doc.headings.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, ["Top", "A", "B"]);
        for h in &doc.headings {
            assert!(doc.lines[h.line].plain.starts_with(&h.title), "{}", h.title);
        }
        assert!(doc.links.is_empty());
        // A blank line stays before the next heading.
        let b = doc.headings[2].line;
        assert_eq!(text[b - 1], "");
    }

    #[test]
    fn math_inline_and_centered_display() {
        let out = plain("Mass $E = mc^2$ here.\n\n$$\n\\alpha \\le \\beta\n$$\n", 30);
        assert_eq!(out[0], "Mass E = mc² here.");
        let d = out.iter().find(|l| l.contains('α')).unwrap();
        assert_eq!(d.trim(), "α ≤ β");
        assert_eq!(d.find('α'), Some((30 - 5) / 2), "{d:?}");
    }

    #[test]
    fn deep_nesting_is_capped() {
        let md = format!("{}start\n{}", "> ".repeat(4000), "abcdefghij klmnopqrst uvwxyz\n".repeat(50));
        let doc = doc_of(&md, 60);
        assert!(doc.width <= 60, "width {}", doc.width);
        assert!(doc.lines.iter().all(|l| l.spans.iter().filter(|s| s.content.starts_with('│')).count() <= MAX_NEST));
        let md = format!("{}- deep item\n", (0..200).map(|i| format!("{}- l{i}\n", "  ".repeat(i))).collect::<String>());
        let doc = doc_of(&md, 60);
        assert!(doc.width <= 64, "width {}", doc.width);
        let deep = doc.lines.iter().find(|l| l.plain.contains("l150")).unwrap();
        assert!(deep.plain.trim_start().starts_with(['•', '◦', '▪']), "{:?}", deep.plain);
    }

    #[test]
    fn html_comments_are_hidden_across_lines() {
        let out = plain("Before\n\n<!--\nTODO: secret note\nsecond line\n-->\n\nAfter\n", 40);
        assert_eq!(out, ["Before", "", "After"]);
        let out = plain("A\n\n<!-- one line -->\n\nB\n", 40);
        assert_eq!(out, ["A", "", "B"]);
        let out = plain("<div>kept <!-- gone --> too</div>\n", 40);
        assert_eq!(out, ["<div>kept  too</div>"]);
    }

    #[test]
    fn metadata_is_hidden_only_at_the_start() {
        assert_eq!(plain("---\ntitle: x\n---\n\n# Body\n", 40)[0], "Body");
        assert_eq!(plain("+++\ntitle = \"x\"\n+++\n\nBody\n", 40), ["Body"]);
        let out = plain("# Title\n\n---\nnot: frontmatter\n---\n\ntext\n", 40);
        assert!(out.iter().any(|l| l == "not: frontmatter"), "{out:#?}");
        assert!(out.iter().any(|l| l == "text"));
    }

    #[test]
    fn huge_tables_are_cut_and_fast() {
        let start = std::time::Instant::now();
        let row = |c: &str| format!("|{}|\n", vec![c; 3000].join("|"));
        let md = format!("{}{}{}", row("h"), row("-"), row(&"x".repeat(50)).repeat(20));
        let doc = doc_of(&md, 100);
        assert!(start.elapsed() < std::time::Duration::from_secs(3), "{:?}", start.elapsed());
        assert!(doc.lines.iter().any(|l| l.plain.contains("table too large")));
        // Normal tables still shrink their widest column to fit.
        let doc = doc_of("| a | b |\n|---|---|\n| short | a much longer cell text here |\n", 30);
        assert!(doc.width <= 30, "{}", doc.width);
    }

    #[test]
    fn wraps_paragraph_at_width() {
        let out = plain("one two three four five six", 10);
        assert_eq!(out, vec!["one two", "three four", "five six"]);
        assert!(out.iter().all(|l| l.width() <= 10));
    }

    #[test]
    fn does_not_split_glued_styles() {
        let out = plain("aaaa foo**bar**", 8);
        assert_eq!(out, vec!["aaaa", "foobar"]);
    }

    #[test]
    fn breaks_long_words() {
        let out = plain("abcdefghijklmnopqrstuvwxy", 10);
        assert_eq!(out, vec!["abcdefghij", "klmnopqrst", "uvwxy"]);
    }

    #[test]
    fn wide_chars_count_double() {
        let out = plain("日本語 日本語", 7);
        assert_eq!(out, vec!["日本語", "日本語"]);
    }

    #[test]
    fn headings_and_blank_lines() {
        let out = plain("# Title\n\ntext\n\n### Sub", 40);
        assert_eq!(out, vec!["Title", "━━━━━", "", "text", "", "### Sub"]);
    }

    #[test]
    fn nested_lists_indent() {
        let out = plain("- a\n  - b\n    - c\n- d", 40);
        assert_eq!(out, vec!["• a", "  ◦ b", "    ▪ c", "• d"]);
    }

    #[test]
    fn ordered_list_and_wrap_indent() {
        let out = plain("1. alpha beta gamma\n2. x", 12);
        assert_eq!(out, vec!["1. alpha", "   beta", "   gamma", "2. x"]);
    }

    #[test]
    fn task_list_markers() {
        let out = plain("- [ ] todo\n- [x] done", 40);
        assert_eq!(out, vec!["☐ todo", "☑ done"]);
    }

    #[test]
    fn blockquote_prefix() {
        let out = plain("> quoted text here", 10);
        assert_eq!(out, vec!["│ quoted", "│ text", "│ here"]);
    }

    #[test]
    fn table_fits_width() {
        let md = "| a | b |\n|---|--:|\n| long cell text | 1 |\n";
        let out = plain(md, 18);
        assert!(out.iter().all(|l| l.width() <= 18), "{out:#?}");
        assert!(out[0].starts_with('┌') && out.last().unwrap().starts_with('└'));
        assert!(out.iter().any(|l| l.starts_with('├')));
    }

    #[test]
    fn table_right_alignment() {
        let out = plain("| n |\n|--:|\n| 1 |\n| 100 |\n", 40);
        assert_eq!(out[3], "│   1 │");
        assert_eq!(out[4], "│ 100 │");
    }

    #[test]
    fn code_block_keeps_width_and_text() {
        let out = plain("```\nlet x = 1;\n```", 20);
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].trim(), "let x = 1;");
    }

    #[test]
    fn links_are_recorded_with_columns() {
        let theme = Theme::new(ThemeKind::Dark);
        let hl = Highlighter::new();
        let mut sizer = NoImages;
        let doc = render("see [here](https://x.y) ok", Ctx { width: 40, theme: &theme, hl: &hl, base_dir: None, sizer: &mut sizer, interactive: true, link_targets: false });
        assert_eq!(doc.links.len(), 1);
        let l = &doc.links[0];
        let text: String = doc.lines[l.line].plain.chars().skip(l.start).take(l.end - l.start).collect();
        assert_eq!(text, "here");
        assert_eq!(l.url, "https://x.y");
    }

    #[test]
    fn wrapped_link_pieces_share_id() {
        let theme = Theme::new(ThemeKind::Dark);
        let hl = Highlighter::new();
        let mut sizer = NoImages;
        let doc = render("aa [bb cc dd](u) [ee](v)", Ctx { width: 8, theme: &theme, hl: &hl, base_dir: None, sizer: &mut sizer, interactive: true, link_targets: false });
        let ids: Vec<usize> = doc.links.iter().map(|l| l.id).collect();
        assert_eq!(ids, vec![0, 0, 1]);
    }

    #[test]
    fn footnote_label_on_same_line() {
        let out = plain("x[^1]\n\n[^1]: note", 40);
        assert_eq!(out.last().unwrap(), "[^1]: note");
    }

    #[test]
    fn badge_image_keeps_outer_link() {
        let theme = Theme::new(ThemeKind::Dark);
        let hl = Highlighter::new();
        let mut sizer = NoImages;
        let doc = render("[![b](https://img/x.svg)](https://target)", Ctx { width: 40, theme: &theme, hl: &hl, base_dir: None, sizer: &mut sizer, interactive: true, link_targets: false });
        assert_eq!(doc.links.len(), 1);
        assert_eq!(doc.links[0].url, "https://target");
    }

    #[test]
    fn empty_list_item_keeps_marker() {
        assert_eq!(plain("-\n- b", 40), vec!["•", "• b"]);
    }

    #[test]
    fn duplicate_headings_get_suffix() {
        let theme = Theme::new(ThemeKind::Dark);
        let hl = Highlighter::new();
        let mut sizer = NoImages;
        let doc = render("# Intro\n# Intro\n# Intro", Ctx { width: 40, theme: &theme, hl: &hl, base_dir: None, sizer: &mut sizer, interactive: true, link_targets: false });
        let slugs: Vec<&str> = doc.headings.iter().map(|h| h.slug.as_str()).collect();
        assert_eq!(slugs, vec!["intro", "intro-1", "intro-2"]);
    }

    #[test]
    fn headings_get_slugs() {
        assert_eq!(slugify("Hello, World! 2"), "hello-world-2");
    }

    #[test]
    fn front_matter_is_hidden() {
        let out = plain("---\ntitle: x\n---\n\nbody", 40);
        assert_eq!(out, vec!["body"]);
    }
}
