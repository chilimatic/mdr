//! Bar, column and line charts drawn with block and braille characters.

use ratatui::style::Style;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{Lines, Palette};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Bar,
    Column,
    Line,
}

#[derive(Debug)]
pub struct Series {
    pub name: Option<String>,
    pub values: Vec<f64>,
}

#[derive(Debug)]
pub struct Chart {
    pub kind: Kind,
    pub title: Option<String>,
    pub labels: Vec<String>,
    pub series: Vec<Series>,
    /// Show each value's share of the series total (pie charts).
    pub percent: bool,
}

const HEIGHT: usize = 10;
const MAX_ROWS: usize = 500;
const MAX_SERIES: usize = 8;

/// Parses a ```chart block:
///
/// ```text
/// type: column        # bar (default), column or line
/// title: Sales
/// series: 2024, 2025  # optional series names
/// Jan: 10, 12
/// Feb: 20, 18
/// ```
pub fn parse_block(src: &str) -> Result<Chart, String> {
    let mut chart = Chart { kind: Kind::Bar, title: None, labels: Vec::new(), series: Vec::new(), percent: false };
    let mut names: Vec<String> = Vec::new();
    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        if let Some((key, val)) = line.split_once(':') {
            let val = val.trim();
            match key.trim().to_ascii_lowercase().as_str() {
                "type" | "kind" => {
                    chart.kind = match val.to_ascii_lowercase().as_str() {
                        "bar" | "hbar" | "barh" => Kind::Bar,
                        "column" | "col" | "vbar" => Kind::Column,
                        "line" => Kind::Line,
                        other => return Err(format!("unknown chart type \"{other}\" (use bar, column or line)")),
                    };
                    continue;
                }
                "title" => {
                    chart.title = Some(val.trim_matches('"').to_string());
                    continue;
                }
                "series" => {
                    names = split_values(val).map(|s| s.trim_matches('"').to_string()).collect();
                    continue;
                }
                _ => {}
            }
        }
        // Labels may contain ':' (like 08:00), so use the last ':' that is followed by numbers.
        let numeric = |r: &str| split_values(r).next().is_some() && split_values(r).all(|v| v.parse::<f64>().is_ok());
        let split = line
            .match_indices(':')
            .rev()
            .map(|(i, _)| (&line[..i], &line[i + 1..]))
            .find(|(_, r)| numeric(r))
            .or_else(|| line.split_once(':'))
            .or_else(|| line.split_once([',', '|']));
        let (label, rest) = split.ok_or_else(|| format!("cannot read chart row \"{line}\""))?;
        let values = split_values(rest)
            .map(|v| v.parse::<f64>().map_err(|_| format!("\"{v}\" is not a number")))
            .collect::<Result<Vec<f64>, String>>()?;
        if values.is_empty() {
            return Err(format!("chart row \"{line}\" has no value"));
        }
        if values.len() > MAX_SERIES {
            return Err(format!("too many series (at most {MAX_SERIES})"));
        }
        if chart.labels.len() >= MAX_ROWS {
            return Err(format!("too many rows (at most {MAX_ROWS})"));
        }
        let row = chart.labels.len();
        chart.labels.push(label.trim().trim_matches('"').to_string());
        for (si, v) in values.into_iter().enumerate() {
            while chart.series.len() <= si {
                chart.series.push(Series { name: names.get(chart.series.len()).cloned(), values: Vec::new() });
            }
            let s = &mut chart.series[si];
            s.values.resize(row, 0.0);
            s.values.push(v);
        }
    }
    Ok(chart)
}

fn split_values(s: &str) -> impl Iterator<Item = &str> {
    s.split([',', '|', ' ', '\t']).map(str::trim).filter(|v| !v.is_empty())
}

pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Shortens `s` to at most `w` columns, measured on the whole string (emoji sequences
/// can be wider than the sum of their chars). Zero-width chars are dropped.
fn trunc(s: &str, w: usize) -> String {
    let s: String = s.chars().filter(|c| c.width().unwrap_or(0) > 0).collect();
    if s.width() <= w {
        return s;
    }
    let mut out = String::new();
    for c in s.chars() {
        out.push(c);
        if out.width() + 1 > w {
            out.pop();
            break;
        }
    }
    if w > 0 {
        out.push('…');
    }
    while out.width() > w {
        out.remove(0);
    }
    out
}

fn pad_right(s: &str, w: usize) -> String {
    format!("{s}{}", " ".repeat(w.saturating_sub(s.width())))
}

fn pad_left(s: &str, w: usize) -> String {
    format!("{}{s}", " ".repeat(w.saturating_sub(s.width())))
}

pub fn render(ch: &Chart, max_w: usize, pal: &Palette) -> Result<Lines, String> {
    if ch.labels.is_empty() || ch.series.is_empty() {
        return Err("chart has no data".into());
    }
    if ch.labels.len() > MAX_ROWS || ch.series.len() > MAX_SERIES {
        return Err(format!("chart too large (at most {MAX_ROWS} rows and {MAX_SERIES} series)"));
    }
    let mut out: Lines = Vec::new();
    if let Some(t) = &ch.title {
        out.push(vec![(trunc(t, max_w), pal.title)]);
    }
    match ch.kind {
        Kind::Bar => bar(ch, max_w, pal, &mut out)?,
        Kind::Column => column(ch, max_w, pal, &mut out)?,
        Kind::Line => line(ch, max_w, pal, &mut out)?,
    }
    if ch.series.len() > 1 || ch.series[0].name.is_some() {
        let mut legend = Vec::new();
        for (i, s) in ch.series.iter().enumerate() {
            let name = s.name.clone().unwrap_or_else(|| format!("series {}", i + 1));
            legend.push(("■ ".to_string(), pal.series(i)));
            legend.push((format!("{name}   "), pal.text));
        }
        out.push(legend);
    }
    Ok(out)
}

const PARTIAL: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

fn bar(ch: &Chart, max_w: usize, pal: &Palette, out: &mut Lines) -> Result<(), String> {
    let label_w = ch.labels.iter().map(|l| l.width()).max().unwrap_or(1).min(max_w / 3).max(1);
    let value = |si: usize, v: f64| -> String {
        if ch.percent {
            let total: f64 = ch.series[si].values.iter().filter(|v| **v > 0.0).sum();
            let pct = if total > 0.0 { v / total * 100.0 } else { 0.0 };
            format!("{} ({pct:.1}%)", fmt_num(v))
        } else {
            fmt_num(v)
        }
    };
    let val_w = ch
        .series
        .iter()
        .enumerate()
        .flat_map(|(si, s)| s.values.iter().map(move |&v| (si, v)))
        .map(|(si, v)| value(si, v).width())
        .max()
        .unwrap_or(1);
    let bar_w = max_w.checked_sub(label_w + val_w + 2).filter(|w| *w >= 4).ok_or("not enough width for this chart")?;
    let maxv = ch.series.iter().flat_map(|s| s.values.iter().copied()).fold(0.0f64, f64::max);
    let maxv = if maxv > 0.0 { maxv } else { 1.0 };
    for (i, label) in ch.labels.iter().enumerate() {
        for (si, s) in ch.series.iter().enumerate() {
            let v = s.values.get(i).copied().unwrap_or(0.0);
            let eighths = (v.max(0.0) / maxv * bar_w as f64 * 8.0).round() as usize;
            let bar = format!("{}{}", "█".repeat(eighths / 8), PARTIAL[eighths % 8]);
            let lab = if si == 0 { pad_right(&trunc(label, label_w), label_w) } else { " ".repeat(label_w) };
            out.push(vec![
                (lab, pal.text),
                (" ".into(), Style::default()),
                (bar.clone(), pal.series(si)),
                (format!(" {}", value(si, v)), pal.dim),
            ]);
        }
    }
    Ok(())
}

const LEVELS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

fn y_axis(top: &str, bottom: &str) -> (usize, impl Fn(usize) -> String + use<>) {
    let w = top.width().max(bottom.width());
    let (top, bottom) = (top.to_string(), bottom.to_string());
    let f = move |row: usize| -> String {
        if row == 0 {
            format!("{} ┤", pad_left(&top, w))
        } else if row == HEIGHT - 1 {
            format!("{} ┤", pad_left(&bottom, w))
        } else {
            format!("{} │", " ".repeat(w))
        }
    };
    (w + 2, f)
}

fn column(ch: &Chart, max_w: usize, pal: &Palette, out: &mut Lines) -> Result<(), String> {
    let n = ch.labels.len();
    let s = ch.series.len();
    let maxv = ch.series.iter().flat_map(|s| s.values.iter().copied()).fold(0.0f64, f64::max);
    let maxv = if maxv > 0.0 { maxv } else { 1.0 };
    let (axis_w, axis) = y_axis(&fmt_num(maxv), "0");
    let avail = max_w.saturating_sub(axis_w);
    let bw = ((avail / n).saturating_sub(1) / s).min(6);
    if bw == 0 {
        return Err("not enough width for this chart".into());
    }
    let group_w = bw * s + 1;
    for row in 0..HEIGHT {
        let level = HEIGHT - 1 - row;
        let mut pieces = vec![(axis(row), pal.dim)];
        for i in 0..n {
            for (si, ser) in ch.series.iter().enumerate() {
                let v = ser.values.get(i).copied().unwrap_or(0.0).max(0.0);
                let total = (v / maxv * (HEIGHT * 8) as f64).round() as usize;
                let fill = total.saturating_sub(level * 8).min(8);
                pieces.push((LEVELS[fill].to_string().repeat(bw), pal.series(si)));
            }
            pieces.push((" ".into(), Style::default()));
        }
        out.push(pieces);
    }
    out.push(vec![(format!("{}└{}", " ".repeat(axis_w - 1), "─".repeat(n * group_w)), pal.dim)]);
    let mut labels = " ".repeat(axis_w);
    for l in &ch.labels {
        let t = trunc(l, group_w - 1);
        let free = group_w.saturating_sub(t.width());
        labels.push_str(&format!("{}{t}{}", " ".repeat(free / 2), " ".repeat(free - free / 2)));
    }
    out.push(vec![(labels.trim_end().to_string(), pal.text)]);
    Ok(())
}

fn line(ch: &Chart, max_w: usize, pal: &Palette, out: &mut Lines) -> Result<(), String> {
    let n = ch.labels.len();
    if n < 2 {
        return Err("a line chart needs at least 2 points".into());
    }
    let all = ch.series.iter().flat_map(|s| s.values.iter().copied());
    let (mut lo, mut hi) = all.fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
    if hi <= lo {
        hi += 1.0;
        lo -= 1.0;
    }
    let (axis_w, axis) = y_axis(&fmt_num(hi), &fmt_num(lo));
    let pw = max_w.saturating_sub(axis_w).min(100);
    if pw < 10 {
        return Err("not enough width for this chart".into());
    }
    let (dw, dh) = (pw * 2, HEIGHT * 4);
    let mut grid = vec![(0u8, 0usize); pw * HEIGHT];
    let mut dot = |x: i64, y: i64, si: usize| {
        if x < 0 || y < 0 || x as usize >= dw || y as usize >= dh {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        const BITS: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];
        let cell = &mut grid[(y / 4) * pw + x / 2];
        cell.0 |= BITS[x % 2][y % 4];
        cell.1 = si;
    };
    for (si, s) in ch.series.iter().enumerate() {
        let pt = |i: usize| -> (i64, i64) {
            let v = s.values.get(i).copied().unwrap_or(lo);
            let x = (i * (dw - 1) / (n - 1)) as i64;
            let y = ((hi - v) / (hi - lo) * (dh - 1) as f64).round() as i64;
            (x, y)
        };
        for i in 0..n - 1 {
            let ((x0, y0), (x1, y1)) = (pt(i), pt(i + 1));
            // Bresenham line between two points.
            let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
            let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
            let (mut x, mut y, mut err) = (x0, y0, dx + dy);
            loop {
                dot(x, y, si);
                if x == x1 && y == y1 {
                    break;
                }
                let e2 = 2 * err;
                if e2 >= dy {
                    err += dy;
                    x += sx;
                }
                if e2 <= dx {
                    err += dx;
                    y += sy;
                }
            }
        }
    }
    for row in 0..HEIGHT {
        let mut pieces: Vec<(String, Style)> = vec![(axis(row), pal.dim)];
        for col in 0..pw {
            let (bits, si) = grid[row * pw + col];
            let (c, st) = if bits == 0 {
                (' ', Style::default())
            } else {
                (char::from_u32(0x2800 + bits as u32).unwrap_or(' '), pal.series(si))
            };
            match pieces.last_mut() {
                Some((s, last)) if *last == st => s.push(c),
                _ => pieces.push((c.to_string(), st)),
            }
        }
        out.push(pieces);
    }
    out.push(vec![(format!("{}└{}", " ".repeat(axis_w - 1), "─".repeat(pw)), pal.dim)]);
    // X labels: first, last, and the middle one if there is room. Positions are in columns.
    let mut used = vec![false; pw];
    let mut placed: Vec<(usize, String)> = Vec::new();
    let mut place = |text: &str, center: usize| {
        let t = trunc(text, pw / 3);
        let w = t.width();
        let start = center.saturating_sub(w / 2).min(pw.saturating_sub(w));
        let span = start.saturating_sub(1)..(start + w + 1).min(pw);
        if used[span.clone()].iter().all(|u| !u) {
            used[span].iter_mut().for_each(|u| *u = true);
            placed.push((start, t));
        }
    };
    place(&ch.labels[0], 0);
    place(&ch.labels[n - 1], pw - 1);
    if n > 2 {
        let mid = n / 2;
        place(&ch.labels[mid], mid * (pw - 1) / (n - 1));
    }
    placed.sort_by_key(|p| p.0);
    let mut labels = String::new();
    for (start, t) in placed {
        labels.push_str(&" ".repeat(start.saturating_sub(labels.width())));
        labels.push_str(&t);
    }
    out.push(vec![(format!("{}{}", " ".repeat(axis_w), labels.trim_end()), pal.text)]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_block_with_series() {
        let c = parse_block("type: column\ntitle: T\nseries: a, b\nJan: 1, 2\nFeb: 3, 4").unwrap();
        assert_eq!(c.kind, Kind::Column);
        assert_eq!(c.labels, vec!["Jan", "Feb"]);
        assert_eq!(c.series.len(), 2);
        assert_eq!(c.series[1].values, vec![2.0, 4.0]);
        assert_eq!(c.series[0].name.as_deref(), Some("a"));
    }

    #[test]
    fn labels_with_colons() {
        let c = parse_block("08:00: 1, 2\n10:30: 3, 4").unwrap();
        assert_eq!(c.labels, vec!["08:00", "10:30"]);
    }

    #[test]
    fn odd_unicode_labels_do_not_panic() {
        use crate::theme::{Theme, ThemeKind};
        let pal = Palette::new(&Theme::new(ThemeKind::Dark));
        let combining = format!("a{}", "\u{301}".repeat(300));
        let src = format!("type: line\n{combining}: 1\nb: 2\nc: 3");
        let _ = render(&parse_block(&src).unwrap(), 80, &pal);
        let emoji = "❤\u{fe0f}".repeat(10);
        let src = format!("type: column\n{emoji}: 1\nb: 2");
        let _ = render(&parse_block(&src).unwrap(), 80, &pal);
        assert!(trunc(&emoji, 5).width() <= 5);
    }

    #[test]
    fn caps_series_count() {
        let row = format!("a: {}", vec!["1"; 50].join(", "));
        assert!(parse_block(&row).is_err());
    }

    #[test]
    fn rejects_bad_number() {
        assert!(parse_block("a: x").is_err());
    }

    #[test]
    fn number_format() {
        assert_eq!(fmt_num(3.0), "3");
        assert_eq!(fmt_num(2.5), "2.5");
        assert_eq!(fmt_num(1.239), "1.24");
    }
}
