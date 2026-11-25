//! Mermaid `classDiagram` and `erDiagram`, drawn as boxes in a top-down graph.

use unicode_width::UnicodeWidthStr;

use super::canvas::Stroke;
use super::chart::trunc;
use super::graph::{self, Dir, Edge, Graph, Shape};

/// Longest member line shown in a box.
const MAX_MEMBER_WIDTH: usize = 60;

/// Box text: optional annotation, name, then groups of member lines under rules.
/// Every line has the same width, so the graph can draw it without trimming.
fn boxed(annotation: Option<&str>, name: &str, groups: &[Vec<String>]) -> String {
    let top = annotation.map(|a| format!("«{a}»"));
    let groups: Vec<Vec<String>> = groups.iter().filter(|g| !g.is_empty()).map(|g| g.iter().map(|m| trunc(m, MAX_MEMBER_WIDTH)).collect()).collect();
    let w = top
        .iter()
        .map(|t| t.width())
        .chain(std::iter::once(name.width()))
        .chain(groups.iter().flatten().map(|m| m.width()))
        .max()
        .unwrap_or(0);
    let center = |s: &str| {
        let left = (w - s.width()) / 2;
        format!("{}{s}{}", " ".repeat(left), " ".repeat(w - s.width() - left))
    };
    let mut lines: Vec<String> = top.iter().map(|t| center(t)).collect();
    lines.push(center(name));
    for g in &groups {
        lines.push("─".repeat(w));
        lines.extend(g.iter().map(|m| format!("{m}{}", " ".repeat(w - m.width()))));
    }
    lines.join("\n")
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(s)
}

/// Splits on whitespace but keeps `"quoted text"` together.
fn tokens(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut quoted) = (None, false);
    for (i, c) in s.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        match (c.is_whitespace() && !quoted, start) {
            (true, Some(st)) => {
                out.push(&s[st..i]);
                start = None;
            }
            (false, None) => start = Some(i),
            _ => {}
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

/// Splits at the first `:` outside quotes.
fn split_colon(s: &str) -> (&str, Option<&str>) {
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return (s[..i].trim(), Some(s[i + 1..].trim())),
            _ => {}
        }
    }
    (s.trim(), None)
}

// ---------------------------------------------------------------------------
// classDiagram
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Class {
    display: String,
    annotation: Option<String>,
    members: Vec<String>,
}

/// `List~T~` becomes `List<T>`, nested generics too.
fn generics(s: &str) -> String {
    // A loop, not recursion: a line of many `~` must not overflow the stack.
    let (mut head, mut rest, mut depth) = (String::new(), s, 0);
    while depth < 8 {
        match rest.find('~') {
            Some(p) if rest.ends_with('~') && rest.len() > p + 1 => {
                head.push_str(&rest[..p]);
                head.push('<');
                rest = &rest[p + 1..rest.len() - 1];
                depth += 1;
            }
            _ => break,
        }
    }
    format!("{head}{rest}{}", ">".repeat(depth))
}

/// Class id (`List`) and display name (`List<T>`).
fn class_name(s: &str) -> (String, String) {
    let s = unquote(s);
    let id = s.split('~').next().unwrap_or(s).to_string();
    (id, generics(s))
}

struct Rel {
    stroke: Stroke,
    head: bool,
    tail: bool,
    kind: &'static str,
    /// The whole is on the right, so the edge is drawn from there.
    swap: bool,
}

fn rel(op: &str) -> Option<Rel> {
    let (stroke, head, tail, kind, swap) = match op {
        "<|--" => (Stroke::Light, false, true, "inherits", false),
        "--|>" => (Stroke::Light, true, false, "inherits", false),
        "<|.." => (Stroke::Dotted, false, true, "implements", false),
        "..|>" => (Stroke::Dotted, true, false, "implements", false),
        "*--" => (Stroke::Light, false, false, "◆ composes", false),
        "--*" => (Stroke::Light, false, false, "◆ composes", true),
        "o--" => (Stroke::Light, false, false, "◇ aggregates", false),
        "--o" => (Stroke::Light, false, false, "◇ aggregates", true),
        "-->" => (Stroke::Light, true, false, "", false),
        "<--" => (Stroke::Light, false, true, "", false),
        "<-->" => (Stroke::Light, true, true, "", false),
        "..>" => (Stroke::Dotted, true, false, "depends on", false),
        "<.." => (Stroke::Dotted, false, true, "depends on", false),
        "--" => (Stroke::Light, false, false, "", false),
        ".." => (Stroke::Dotted, false, false, "", false),
        _ => return None,
    };
    Some(Rel { stroke, head, tail, kind, swap })
}

fn class_index(g: &mut Graph, classes: &mut Vec<Class>, raw: &str) -> Result<usize, String> {
    let (id, display) = class_name(raw);
    if id.is_empty() {
        return Err("empty class name".into());
    }
    let before = g.nodes.len();
    let i = g.node(&id);
    if g.nodes.len() > before {
        classes.push(Class { display: id.clone(), ..Class::default() });
    }
    if display != id {
        classes[i].display = display;
    }
    Ok(i)
}

fn add_member(c: &mut Class, text: &str) {
    let text = text.trim();
    if let Some(a) = text.strip_prefix("<<").and_then(|t| t.strip_suffix(">>")) {
        c.annotation = Some(a.trim().to_string());
    } else if !text.is_empty() {
        c.members.push(text.split(' ').map(generics).collect::<Vec<_>>().join(" "));
    }
}

const CLASS_SKIP: [&str; 12] = ["direction", "note", "classDef", "style", "cssClass", "link", "callback", "click", "namespace", "accTitle", "accDescr", "title"];

/// `A "1" --> "*" B : label`
fn parse_relation(g: &mut Graph, classes: &mut Vec<Class>, line: &str) -> Result<bool, String> {
    let (head, label) = split_colon(line);
    let toks = tokens(head);
    let Some(pos) = toks.iter().position(|t| rel(t).is_some()) else {
        return Ok(false);
    };
    let r = rel(toks[pos]).expect("checked above");
    let (left, right) = (&toks[..pos], &toks[pos + 1..]);
    let (Some(&a), Some(&b)) = (left.first(), right.last()) else {
        return Ok(false);
    };
    let card = |t: Option<&&str>| t.filter(|t| t.starts_with('"')).map(|t| unquote(t).to_string());
    let (mut ca, mut cb) = (if left.len() > 1 { card(left.last()) } else { None }, if right.len() > 1 { card(right.first()) } else { None });
    let (mut from, mut to) = (class_index(g, classes, a)?, class_index(g, classes, b)?);
    if r.swap {
        std::mem::swap(&mut from, &mut to);
        std::mem::swap(&mut ca, &mut cb);
    }
    let mut text = String::new();
    for part in [r.kind, label.map(unquote).unwrap_or("")] {
        if !part.is_empty() {
            if !text.is_empty() {
                text.push_str(" · ");
            }
            text.push_str(part);
        }
    }
    if ca.is_some() || cb.is_some() {
        let cards = format!("{} → {}", ca.as_deref().unwrap_or("-"), cb.as_deref().unwrap_or("-"));
        text = if text.is_empty() { cards } else { format!("{text} ({cards})") };
    }
    if g.edges.len() >= graph::MAX_EDGES {
        return Err("diagram too large".into());
    }
    g.edges.push(Edge { from, to, label: (!text.is_empty()).then_some(text), stroke: r.stroke, head: r.head, tail: r.tail });
    Ok(true)
}

pub fn parse_class(body: &[&str]) -> Result<Graph, String> {
    let mut g = Graph::new(Dir::TopDown);
    g.raw_labels = true;
    let mut classes: Vec<Class> = Vec::new();
    let mut open: Option<usize> = None;
    for &line in body {
        if g.nodes.len() > graph::MAX_NODES {
            return Err("diagram too large".into());
        }
        if let Some(i) = open {
            if line.starts_with('}') {
                open = None;
            } else {
                add_member(&mut classes[i], line);
            }
            continue;
        }
        let first = line.split(|c: char| c.is_whitespace() || c == ':').next().unwrap_or("");
        if line.starts_with('}') || CLASS_SKIP.contains(&first) {
            continue;
        }
        if first == "class" {
            let mut rest = line["class".len()..].trim();
            let opens = rest.ends_with('{');
            rest = rest.trim_end_matches('{').trim();
            if let Some((name, _)) = rest.split_once(":::") {
                rest = name.trim();
            }
            let (name, label) = match rest.split_once('[') {
                Some((n, l)) => (n.trim(), Some(unquote(l.trim_end_matches(']')).to_string())),
                None => (rest, None),
            };
            if name.is_empty() {
                continue;
            }
            let i = class_index(&mut g, &mut classes, name)?;
            if let Some(l) = label {
                classes[i].display = l;
            }
            if opens {
                open = Some(i);
            }
        } else if line.starts_with("<<") {
            if let Some((ann, name)) = line.split_once(">>") {
                let i = class_index(&mut g, &mut classes, name.trim())?;
                classes[i].annotation = Some(ann[2..].trim().to_string());
            }
        } else if !parse_relation(&mut g, &mut classes, line)?
            && let (name, Some(member)) = split_colon(line)
            && !name.is_empty()
            && !name.contains(' ')
        {
            let i = class_index(&mut g, &mut classes, name)?;
            add_member(&mut classes[i], member);
        }
    }
    if g.nodes.is_empty() {
        return Err("class diagram has no classes".into());
    }
    for (i, c) in classes.iter().enumerate() {
        let (attrs, methods): (Vec<String>, Vec<String>) = c.members.iter().cloned().partition(|m| !m.contains('('));
        g.nodes[i].label = boxed(c.annotation.as_deref(), &c.display, &[attrs, methods]);
        g.nodes[i].shape = Shape::Rect;
    }
    Ok(g)
}

// ---------------------------------------------------------------------------
// erDiagram
// ---------------------------------------------------------------------------

fn er_left(s: &str) -> Option<&'static str> {
    Some(match s {
        "||" => "1",
        "|o" => "0..1",
        "}o" => "0..n",
        "}|" => "1..n",
        _ => return None,
    })
}

fn er_right(s: &str) -> Option<&'static str> {
    Some(match s {
        "||" => "1",
        "o|" => "0..1",
        "o{" => "0..n",
        "|{" => "1..n",
        _ => return None,
    })
}

/// `A ||--o{ B`: cardinalities and whether the line is dotted.
fn er_op(t: &str) -> Option<(&'static str, &'static str, bool)> {
    let c: Vec<char> = t.chars().collect();
    if c.len() != 6 {
        return None;
    }
    let part = |r: std::ops::Range<usize>| c[r].iter().collect::<String>();
    let dotted = match part(2..4).as_str() {
        "--" => false,
        ".." => true,
        _ => return None,
    };
    Some((er_left(&part(0..2))?, er_right(&part(4..6))?, dotted))
}

/// `type name PK, FK "comment"` as `name: type PK,FK`.
fn er_attribute(line: &str) -> Option<String> {
    let head = line.split('"').next().unwrap_or(line);
    let mut t = head.split_whitespace();
    let (ty, name) = (t.next()?, t.next()?);
    let keys: Vec<&str> = t.flat_map(|k| k.split(',')).filter(|k| !k.is_empty()).collect();
    Some(if keys.is_empty() { format!("{name}: {ty}") } else { format!("{name}: {ty} {}", keys.join(",")) })
}

pub fn parse_er(body: &[&str]) -> Result<Graph, String> {
    let mut g = Graph::new(Dir::TopDown);
    g.raw_labels = true;
    let mut attrs: Vec<Vec<String>> = Vec::new();
    let mut open: Option<usize> = None;
    let entity = |g: &mut Graph, attrs: &mut Vec<Vec<String>>, raw: &str| {
        let before = g.nodes.len();
        let i = g.node(unquote(raw));
        if g.nodes.len() > before {
            attrs.push(Vec::new());
        }
        i
    };
    for &line in body {
        if g.nodes.len() > graph::MAX_NODES {
            return Err("diagram too large".into());
        }
        if let Some(i) = open {
            if line.starts_with('}') {
                open = None;
            } else if let Some(a) = er_attribute(line) {
                attrs[i].push(a);
            }
            continue;
        }
        let (head, label) = split_colon(line);
        let toks = tokens(head);
        if let Some(pos) = toks.iter().position(|t| er_op(t).is_some())
            && pos > 0
            && pos + 2 == toks.len()
        {
            let (ca, cb, dotted) = er_op(toks[pos]).expect("checked above");
            let (from, to) = (entity(&mut g, &mut attrs, toks[0]), entity(&mut g, &mut attrs, toks[pos + 1]));
            let cards = format!("({ca} to {cb})");
            let text = match label.map(unquote).filter(|l| !l.is_empty()) {
                Some(l) => format!("{l} {cards}"),
                None => cards,
            };
            if g.edges.len() >= graph::MAX_EDGES {
                return Err("diagram too large".into());
            }
            g.edges.push(Edge { from, to, label: Some(text), stroke: if dotted { Stroke::Dotted } else { Stroke::Light }, head: false, tail: false });
        } else if let Some(name) = line.strip_suffix('{') {
            let name = name.trim().split('[').next().unwrap_or("").trim();
            if !name.is_empty() {
                open = Some(entity(&mut g, &mut attrs, name));
            }
        } else if toks.len() == 1 && label.is_none() {
            entity(&mut g, &mut attrs, toks[0]);
        }
    }
    if g.nodes.is_empty() {
        return Err("ER diagram has no entities".into());
    }
    for (i, a) in attrs.into_iter().enumerate() {
        let name = g.nodes[i].id.clone();
        g.nodes[i].label = boxed(None, &name, &[a]);
    }
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generics_nest_and_survive_long_input() {
        assert_eq!(generics("List~T~"), "List<T>");
        assert_eq!(generics("Map~K, List~V~~"), "Map<K, List<V>>");
        let _ = generics(&"~".repeat(60_000));
    }
}
