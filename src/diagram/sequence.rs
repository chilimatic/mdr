//! Mermaid sequence diagrams: participants as columns, messages as arrows between lifelines.

use unicode_width::UnicodeWidthStr;

use super::canvas::{Canvas, Stroke};
use super::{Lines, Palette};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Head {
    Arrow,
    Open,
    Cross,
    Async,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
    Over,
}

#[derive(Debug)]
pub enum Ev {
    Msg { from: usize, to: usize, text: String, dotted: bool, head: Head },
    Note { a: usize, b: usize, side: Side, text: String },
    Block { kind: String, text: String },
    End,
}

#[derive(Debug, Default)]
pub struct Seq {
    pub parts: Vec<(String, String)>,
    pub events: Vec<Ev>,
    pub autonumber: bool,
}

impl Seq {
    fn part(&mut self, id: &str) -> usize {
        let id = id.trim();
        if let Some(i) = self.parts.iter().position(|p| p.0 == id) {
            return i;
        }
        self.parts.push((id.to_string(), id.to_string()));
        self.parts.len() - 1
    }
}

const MAX_EVENTS: usize = 1000;
const MAX_PARTS: usize = 50;

const ARROWS: [&str; 8] = ["-->>", "->>", "--x", "-x", "--)", "-)", "-->", "->"];

pub fn parse(body: &[&str]) -> Result<Seq, String> {
    let mut sq = Seq::default();
    let mut stack: Vec<bool> = Vec::new(); // true = block that renders, false = box/rect
    for &raw in body {
        let mut line = raw.trim();
        if let Some(rest) = line.strip_prefix("create ") {
            line = rest.trim();
        }
        let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match word.to_ascii_lowercase().as_str() {
            "participant" | "actor" => {
                let (id, label) = match rest.split_once(" as ") {
                    Some((id, label)) => (id.trim(), label.trim()),
                    None => (rest, rest),
                };
                let i = sq.part(id);
                sq.parts[i].1 = label.to_string();
                continue;
            }
            "autonumber" => {
                sq.autonumber = true;
                continue;
            }
            "activate" | "deactivate" | "title" | "link" | "links" | "properties" | "details" | "destroy" => continue,
            "box" | "rect" => {
                stack.push(false);
                continue;
            }
            "loop" | "alt" | "opt" | "par" | "critical" | "break" => {
                stack.push(true);
                sq.events.push(Ev::Block { kind: word.to_ascii_lowercase(), text: rest.to_string() });
                continue;
            }
            "else" | "and" | "option" => {
                sq.events.push(Ev::Block { kind: word.to_ascii_lowercase(), text: rest.to_string() });
                continue;
            }
            "end" => {
                if stack.pop() == Some(true) {
                    sq.events.push(Ev::End);
                }
                continue;
            }
            "note" => {
                let lower = rest.to_ascii_lowercase();
                let (side, after) = if lower.starts_with("left of") {
                    (Side::Left, &rest[7..])
                } else if lower.starts_with("right of") {
                    (Side::Right, &rest[8..])
                } else if lower.starts_with("over") {
                    (Side::Over, &rest[4..])
                } else {
                    return Err(format!("cannot parse note \"{line}\""));
                };
                let (who, text) = after.split_once(':').ok_or_else(|| format!("note without text: \"{line}\""))?;
                let (a, b) = match who.split_once(',') {
                    Some((a, b)) => (sq.part(a), sq.part(b)),
                    None => {
                        let a = sq.part(who);
                        (a, a)
                    }
                };
                sq.events.push(Ev::Note { a, b, side, text: text.trim().to_string() });
                continue;
            }
            _ => {}
        }
        // Message: find the first arrow; at the same position prefer the longest one.
        let found = ARROWS
            .iter()
            .filter_map(|a| line.find(a).map(|p| (p, *a)))
            .min_by(|x, y| x.0.cmp(&y.0).then(y.1.len().cmp(&x.1.len())));
        let Some((p, arrow)) = found else {
            return Err(format!("cannot parse \"{line}\""));
        };
        let from = &line[..p];
        let after = &line[p + arrow.len()..];
        let (to, text) = after.split_once(':').unwrap_or((after, ""));
        let to = to.trim().trim_start_matches(['+', '-']);
        if from.trim().is_empty() || to.is_empty() {
            return Err(format!("cannot parse \"{line}\""));
        }
        let head = if arrow.ends_with(">>") {
            Head::Arrow
        } else if arrow.ends_with('x') {
            Head::Cross
        } else if arrow.ends_with(')') {
            Head::Async
        } else {
            Head::Open
        };
        let (from, to) = (sq.part(from), sq.part(to));
        sq.events.push(Ev::Msg { from, to, text: text.trim().to_string(), dotted: arrow.starts_with("--"), head });
    }
    if sq.parts.is_empty() {
        return Err("sequence diagram has no participants".into());
    }
    if sq.events.len() > MAX_EVENTS || sq.parts.len() > MAX_PARTS {
        return Err(format!("sequence diagram too large (at most {MAX_EVENTS} messages and {MAX_PARTS} participants)"));
    }
    Ok(sq)
}

pub fn render(sq: &Seq, max_w: usize, pal: &Palette) -> Result<Lines, String> {
    let np = sq.parts.len();
    let bw: Vec<usize> = sq.parts.iter().map(|p| p.1.width() + 4).collect();
    let mut gap: Vec<usize> = (0..np.saturating_sub(1)).map(|i| (bw[i] + bw[i + 1]) / 2 + 3).collect();
    let mut left = bw[0] / 2;
    let mut right = bw[np - 1] - bw[np - 1] / 2;
    let num_w = if sq.autonumber { format!("{}. ", sq.events.len()).len() } else { 0 };
    let mut min_total = 0;

    let need_between = |gap: &mut Vec<usize>, a: usize, b: usize, need: usize| {
        let (lo, hi) = (a.min(b), a.max(b));
        let dist: usize = gap[lo..hi].iter().sum();
        if dist < need {
            gap[hi - 1] += need - dist;
        }
    };
    for ev in &sq.events {
        match ev {
            Ev::Msg { from, to, text, .. } => {
                let tw = text.width() + num_w;
                if from != to {
                    need_between(&mut gap, *from, *to, tw + 4);
                } else if from + 1 < np {
                    gap[*from] = gap[*from].max(tw + 6);
                } else {
                    right = right.max(tw + 6);
                }
            }
            Ev::Note { a, b, side, text } => {
                let w = text.width() + 4;
                match side {
                    Side::Over if a != b => need_between(&mut gap, *a, *b, w.saturating_sub(4)),
                    Side::Over => {
                        let half = w / 2 + 1;
                        if *a == 0 { left = left.max(half) } else { gap[a - 1] = gap[a - 1].max(half + bw[a - 1] / 2) }
                        if *a + 1 == np { right = right.max(half) } else { gap[*a] = gap[*a].max(half + bw[a + 1] / 2) }
                    }
                    Side::Right => {
                        if *a + 1 == np { right = right.max(w + 3) } else { gap[*a] = gap[*a].max(w + 3) }
                    }
                    Side::Left => {
                        if *a == 0 { left = left.max(w + 3) } else { gap[a - 1] = gap[a - 1].max(w + 3) }
                    }
                }
            }
            Ev::Block { kind, text } => min_total = min_total.max(kind.width() + text.width() + 6),
            Ev::End => {}
        }
    }
    let mut cx = vec![left; np];
    for i in 1..np {
        cx[i] = cx[i - 1] + gap[i - 1];
    }
    let width = (cx[np - 1] + right).max(min_total);
    if width > max_w {
        return Err(format!("diagram needs {width} columns, {max_w} available"));
    }
    let body: usize = sq
        .events
        .iter()
        .map(|e| match e {
            Ev::Msg { .. } => 2,
            Ev::Note { .. } => 3,
            _ => 1,
        })
        .sum();
    let height = 3 + body + 3;
    let mut c = Canvas::new(width, height);

    for &x in &cx {
        c.line(x, 3, x, height - 4, Stroke::Dotted, pal.edge);
    }
    for (i, p) in sq.parts.iter().enumerate() {
        let x = cx[i] - bw[i] / 2;
        for y in [0, height - 3] {
            c.frame(x, y, bw[i], 3, ['┌', '┐', '└', '┘'], pal.border);
            c.text(x + 2, y + 1, &p.1, pal.text);
        }
    }

    let mut y = 3;
    let mut num = 1;
    for ev in &sq.events {
        match ev {
            Ev::Msg { from, to, text, dotted, head } => {
                let text = if sq.autonumber {
                    num += 1;
                    format!("{}. {text}", num - 1)
                } else {
                    text.clone()
                };
                let stroke = if *dotted { Stroke::Dotted } else { Stroke::Light };
                let (a, b) = (cx[*from], cx[*to]);
                if from != to {
                    let tw = text.width();
                    let mid = (a + b) / 2;
                    c.text(mid.saturating_sub(tw / 2), y, &text, pal.label);
                    c.line(a, y + 1, b, y + 1, stroke, pal.edge);
                    let (hx, glyph) = if b > a {
                        (b - 1, match head { Head::Cross => '×', Head::Async => '›', _ => '▶' })
                    } else {
                        (b + 1, match head { Head::Cross => '×', Head::Async => '‹', _ => '◀' })
                    };
                    if *head != Head::Open {
                        c.put(hx, y + 1, glyph, pal.arrow);
                    }
                } else {
                    c.line(a, y, a + 3, y, stroke, pal.edge);
                    c.line(a + 3, y, a + 3, y + 1, stroke, pal.edge);
                    c.line(a + 3, y + 1, a + 1, y + 1, stroke, pal.edge);
                    if *head != Head::Open {
                        c.put(a + 1, y + 1, '◀', pal.arrow);
                    }
                    c.text(a + 5, y, &text, pal.label);
                }
                y += 2;
            }
            Ev::Note { a, b, side, text } => {
                let tw = text.width();
                let (x0, w) = match side {
                    Side::Over => {
                        let (lo, hi) = (cx[*a].min(cx[*b]), cx[*a].max(cx[*b]));
                        let w = (hi - lo + 5).max(tw + 4);
                        ((lo + hi) / 2 - w / 2, w)
                    }
                    Side::Right => (cx[*a] + 2, tw + 4),
                    Side::Left => (cx[*a].saturating_sub(tw + 6), tw + 4),
                };
                c.text(x0 + 1, y + 1, &" ".repeat(w - 2), pal.text);
                c.frame(x0, y, w, 3, ['┌', '┐', '└', '┘'], pal.dim);
                c.text(x0 + (w - tw) / 2, y + 1, text, pal.text);
                y += 3;
            }
            Ev::Block { kind, text } => {
                c.line(0, y, width - 1, y, Stroke::Dotted, pal.dim);
                c.text(1, y, &format!(" [{kind}] {text} ").replace("  ", " "), pal.label);
                y += 1;
            }
            Ev::End => {
                c.line(0, y, width - 1, y, Stroke::Dotted, pal.dim);
                y += 1;
            }
        }
    }
    Ok(c.lines())
}
