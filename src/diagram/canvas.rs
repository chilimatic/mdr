//! A character grid that merges box-drawing lines into the right junction glyphs.

use ratatui::style::Style;
use unicode_width::UnicodeWidthChar;

pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
pub const LEFT: u8 = 4;
pub const RIGHT: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stroke {
    Light,
    Heavy,
    Dotted,
}

#[derive(Clone)]
struct Cell {
    /// Fixed text. Lines never draw over it.
    ch: Option<char>,
    /// Second half of a wide character.
    cont: bool,
    bits: u8,
    stroke: Stroke,
    style: Style,
}

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    cells: Vec<Cell>,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Self {
        let blank = Cell { ch: None, cont: false, bits: 0, stroke: Stroke::Light, style: Style::default() };
        Canvas { w, h, cells: vec![blank; w * h] }
    }

    fn cell(&mut self, x: usize, y: usize) -> Option<&mut Cell> {
        if x < self.w && y < self.h { Some(&mut self.cells[y * self.w + x]) } else { None }
    }

    /// Writes text. Text replaces lines and is never replaced by lines.
    pub fn text(&mut self, x: usize, y: usize, s: &str, style: Style) {
        let mut cx = x;
        for c in s.chars() {
            let cw = c.width().unwrap_or(0);
            if cw == 0 {
                continue;
            }
            if cx + cw > self.w {
                break;
            }
            if let Some(cell) = self.cell(cx, y) {
                *cell = Cell { ch: Some(c), cont: false, bits: 0, stroke: Stroke::Light, style };
            }
            if cw == 2
                && let Some(cell) = self.cell(cx + 1, y)
            {
                *cell = Cell { ch: None, cont: true, bits: 0, stroke: Stroke::Light, style };
            }
            cx += cw;
        }
    }

    pub fn put(&mut self, x: usize, y: usize, c: char, style: Style) {
        let mut buf = [0u8; 4];
        self.text(x, y, c.encode_utf8(&mut buf), style);
    }

    fn add_bits(&mut self, x: usize, y: usize, bits: u8, stroke: Stroke, style: Style) {
        if let Some(cell) = self.cell(x, y)
            && cell.ch.is_none()
            && !cell.cont
        {
            cell.bits |= bits;
            if stroke != Stroke::Light || cell.stroke == Stroke::Light {
                cell.stroke = stroke;
            }
            cell.style = style;
        }
    }

    /// Draws a horizontal or vertical line. End cells get half a line, so lines that meet form corners.
    pub fn line(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, stroke: Stroke, style: Style) {
        if y0 == y1 {
            let (a, b) = (x0.min(x1), x0.max(x1));
            for x in a..=b {
                let bits = if x > a { LEFT } else { 0 } | if x < b { RIGHT } else { 0 };
                self.add_bits(x, y0, if a == b { LEFT | RIGHT } else { bits }, stroke, style);
            }
        } else if x0 == x1 {
            let (a, b) = (y0.min(y1), y0.max(y1));
            for y in a..=b {
                let bits = if y > a { UP } else { 0 } | if y < b { DOWN } else { 0 };
                self.add_bits(x0, y, if a == b { UP | DOWN } else { bits }, stroke, style);
            }
        }
    }

    /// Draws a box outline with the given corner set: `[tl, tr, bl, br]`.
    pub fn frame(&mut self, x: usize, y: usize, w: usize, h: usize, corners: [char; 4], style: Style) {
        if w < 2 || h < 2 {
            return;
        }
        let horiz: String = "─".repeat(w - 2);
        self.text(x, y, &format!("{}{horiz}{}", corners[0], corners[1]), style);
        self.text(x, y + h - 1, &format!("{}{horiz}{}", corners[2], corners[3]), style);
        for yy in y + 1..y + h - 1 {
            self.put(x, yy, '│', style);
            self.put(x + w - 1, yy, '│', style);
        }
    }

    /// Rows of (text, style) pieces, with trailing blanks removed.
    pub fn lines(&self) -> Vec<Vec<(String, Style)>> {
        let mut out = Vec::with_capacity(self.h);
        for y in 0..self.h {
            let row = &self.cells[y * self.w..(y + 1) * self.w];
            let last = row.iter().rposition(|c| c.ch.is_some() || c.cont || c.bits != 0).map_or(0, |i| i + 1);
            let mut pieces: Vec<(String, Style)> = Vec::new();
            for cell in &row[..last] {
                if cell.cont {
                    continue;
                }
                let (ch, style) = match cell.ch {
                    Some(c) => (c, cell.style),
                    None if cell.bits != 0 => (glyph(cell.bits, cell.stroke), cell.style),
                    None => (' ', Style::default()),
                };
                match pieces.last_mut() {
                    Some((s, st)) if *st == style => s.push(ch),
                    _ => pieces.push((ch.to_string(), style)),
                }
            }
            out.push(pieces);
        }
        out
    }
}

fn glyph(bits: u8, stroke: Stroke) -> char {
    let v = bits & (UP | DOWN) != 0;
    let h = bits & (LEFT | RIGHT) != 0;
    let heavy = stroke == Stroke::Heavy;
    if stroke == Stroke::Dotted && v != h {
        return if v { '┆' } else { '┄' };
    }
    let light = match bits {
        b if b == UP | DOWN || b == UP || b == DOWN => '│',
        b if b == LEFT | RIGHT || b == LEFT || b == RIGHT => '─',
        b if b == DOWN | RIGHT => '┌',
        b if b == DOWN | LEFT => '┐',
        b if b == UP | RIGHT => '└',
        b if b == UP | LEFT => '┘',
        b if b == UP | DOWN | RIGHT => '├',
        b if b == UP | DOWN | LEFT => '┤',
        b if b == DOWN | LEFT | RIGHT => '┬',
        b if b == UP | LEFT | RIGHT => '┴',
        _ => '┼',
    };
    if !heavy {
        return light;
    }
    match light {
        '│' => '┃',
        '─' => '━',
        '┌' => '┏',
        '┐' => '┓',
        '└' => '┗',
        '┘' => '┛',
        '├' => '┣',
        '┤' => '┫',
        '┬' => '┳',
        '┴' => '┻',
        _ => '╋',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(c: &Canvas) -> Vec<String> {
        c.lines().iter().map(|l| l.iter().map(|p| p.0.as_str()).collect()).collect()
    }

    #[test]
    fn lines_merge_into_junctions() {
        let mut c = Canvas::new(5, 3);
        c.line(0, 1, 4, 1, Stroke::Light, Style::default());
        c.line(2, 0, 2, 2, Stroke::Light, Style::default());
        assert_eq!(plain(&c), vec!["  │", "──┼──", "  │"]);
    }

    #[test]
    fn corners_from_line_ends() {
        let mut c = Canvas::new(3, 3);
        c.line(0, 0, 0, 2, Stroke::Light, Style::default());
        c.line(0, 2, 2, 2, Stroke::Light, Style::default());
        assert_eq!(plain(&c), vec!["│", "│", "└──"]);
    }

    #[test]
    fn text_blocks_lines() {
        let mut c = Canvas::new(5, 1);
        c.text(2, 0, "x", Style::default());
        c.line(0, 0, 4, 0, Stroke::Light, Style::default());
        assert_eq!(plain(&c), vec!["──x──"]);
    }
}
