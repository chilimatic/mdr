//! Parser for a practical subset of Mermaid: flowchart/graph, stateDiagram,
//! sequenceDiagram, pie and xychart.

use super::canvas::Stroke;
use super::chart::{self, Chart, Kind, Series};
use super::graph::{self, Dir, Edge, Graph, Shape};
use super::{Lines, Palette, sequence};

pub fn render(src: &str, width: usize, pal: &Palette) -> Result<Lines, String> {
    let mut lines: Vec<&str> = Vec::new();
    let mut in_front_matter = false;
    for (i, raw) in src.lines().enumerate() {
        let t = raw.trim();
        if t == "---" && (i == 0 || in_front_matter) {
            in_front_matter = !in_front_matter;
            continue;
        }
        if in_front_matter || t.is_empty() || t.starts_with("%%") {
            continue;
        }
        lines.push(t);
    }
    let Some((&header, body)) = lines.split_first() else {
        return Err("empty mermaid diagram".into());
    };
    let kind = header.split_whitespace().next().unwrap_or("");
    match kind {
        "graph" | "flowchart" => {
            let g = parse_flowchart(header, body)?;
            graph::render(&g, super::MAX_DIAGRAM_WIDTH, pal).map(|c| c.lines())
        }
        "stateDiagram" | "stateDiagram-v2" => {
            let g = parse_state(body)?;
            graph::render(&g, super::MAX_DIAGRAM_WIDTH, pal).map(|c| c.lines())
        }
        "sequenceDiagram" => sequence::render(&sequence::parse(body)?, super::MAX_DIAGRAM_WIDTH, pal),
        "pie" => chart::render(&parse_pie(header, body)?, width, pal),
        "xychart-beta" | "xychart" => chart::render(&parse_xychart(header, body)?, width, pal),
        other => Err(format!("mermaid \"{other}\" diagrams are not supported")),
    }
}

// ---------------------------------------------------------------------------
// Flowchart
// ---------------------------------------------------------------------------

fn parse_dir(s: &str) -> Dir {
    match s.to_ascii_uppercase().as_str() {
        "LR" => Dir::LeftRight,
        "RL" => Dir::RightLeft,
        "BT" => Dir::BottomUp,
        _ => Dir::TopDown,
    }
}

const SKIP_WORDS: [&str; 9] = ["subgraph", "end", "classDef", "class", "style", "linkStyle", "click", "direction", "accTitle"];

fn parse_flowchart(header: &str, body: &[&str]) -> Result<Graph, String> {
    let mut parts = header.splitn(2, ';');
    let head = parts.next().unwrap_or("");
    let dir = parse_dir(head.split_whitespace().nth(1).unwrap_or("TD"));
    let mut g = Graph::new(dir);
    let extra = parts.next().into_iter();
    for line in extra.chain(body.iter().copied()) {
        for stmt in split_statements(line) {
            let stmt = stmt.trim();
            let first = stmt.split_whitespace().next().unwrap_or("");
            if stmt.is_empty() || SKIP_WORDS.contains(&first) || first.starts_with("accDescr") {
                continue;
            }
            parse_statement(&mut g, stmt)?;
        }
    }
    if g.nodes.is_empty() {
        return Err("flowchart has no nodes".into());
    }
    Ok(g)
}

/// Splits on `;` outside of brackets and quotes.
fn split_statements(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut quoted, mut start) = (0i32, false, 0);
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth -= 1,
            ';' if !quoted && depth <= 0 => {
                out.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&line[start..]);
    out
}

struct Cur {
    c: Vec<char>,
    i: usize,
}

impl Cur {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }
    fn at(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }
    fn starts(&self, s: &str) -> bool {
        s.chars().enumerate().all(|(k, ch)| self.at(k) == Some(ch))
    }
    fn eat(&mut self, s: &str) -> bool {
        if self.starts(s) {
            self.i += s.chars().count();
            true
        } else {
            false
        }
    }
    fn ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }
    fn eof(&self) -> bool {
        self.i >= self.c.len()
    }
}

struct Link {
    stroke: Stroke,
    head: bool,
    tail: bool,
    label: Option<String>,
}

fn parse_statement(g: &mut Graph, s: &str) -> Result<(), String> {
    let mut c = Cur { c: s.chars().collect(), i: 0 };
    let mut prev = parse_group(g, &mut c, s)?;
    loop {
        c.ws();
        if c.eof() {
            return Ok(());
        }
        let link = parse_link(&mut c).ok_or_else(|| format!("cannot parse \"{s}\""))?;
        c.ws();
        let next = parse_group(g, &mut c, s)?;
        if g.edges.len() + prev.len() * next.len() > graph::MAX_EDGES || g.nodes.len() > graph::MAX_NODES {
            return Err("diagram too large".into());
        }
        for &a in &prev {
            for &b in &next {
                g.edges.push(Edge { from: a, to: b, label: link.label.clone(), stroke: link.stroke, head: link.head, tail: link.tail });
            }
        }
        prev = next;
    }
}

fn parse_group(g: &mut Graph, c: &mut Cur, s: &str) -> Result<Vec<usize>, String> {
    let mut v = vec![parse_node(g, c, s)?];
    loop {
        c.ws();
        if c.eat("&") {
            c.ws();
            v.push(parse_node(g, c, s)?);
        } else {
            return Ok(v);
        }
    }
}

const SHAPES: [(&str, &str, Shape); 14] = [
    ("(((", ")))", Shape::Round),
    ("((", "))", Shape::Round),
    ("([", "])", Shape::Round),
    ("[[", "]]", Shape::Rect),
    ("[(", ")]", Shape::Round),
    ("[/", "/]", Shape::Rect),
    ("[/", "\\]", Shape::Rect),
    ("[\\", "\\]", Shape::Rect),
    ("[\\", "/]", Shape::Rect),
    ("{{", "}}", Shape::Diamond),
    ("[", "]", Shape::Rect),
    ("(", ")", Shape::Round),
    ("{", "}", Shape::Diamond),
    (">", "]", Shape::Rect),
];

fn parse_node(g: &mut Graph, c: &mut Cur, s: &str) -> Result<usize, String> {
    let start = c.i;
    while let Some(ch) = c.peek() {
        let link_next = matches!(c.at(1), Some('-' | '.' | '>' | '='));
        if ch.is_alphanumeric() || ch == '_' || (ch == '-' && c.i > start && !link_next) {
            c.i += 1;
        } else {
            break;
        }
    }
    let id: String = c.c[start..c.i].iter().collect();
    if id.is_empty() {
        return Err(format!("expected a node in \"{s}\""));
    }
    for (open, close, shape) in SHAPES {
        if !c.starts(open) {
            continue;
        }
        let save = c.i;
        c.i += open.chars().count();
        if let Some(text) = read_until(c, close) {
            let label = clean_label(&text);
            let i = g.set_node(&id, Some(label), Some(shape));
            skip_class(c);
            return Ok(i);
        }
        c.i = save;
    }
    skip_class(c);
    Ok(g.node(&id))
}

fn skip_class(c: &mut Cur) {
    if c.eat(":::") {
        while c.peek().is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-') {
            c.i += 1;
        }
    }
}

fn read_until(c: &mut Cur, close: &str) -> Option<String> {
    let mut text = String::new();
    c.ws();
    if c.peek() == Some('"') {
        c.i += 1;
        while let Some(ch) = c.peek() {
            c.i += 1;
            if ch == '"' {
                break;
            }
            text.push(ch);
        }
        c.ws();
        return c.eat(close).then_some(text);
    }
    while !c.eof() {
        if c.starts(close) {
            c.i += close.chars().count();
            return Some(text);
        }
        text.push(c.peek()?);
        c.i += 1;
    }
    None
}

fn clean_label(s: &str) -> String {
    let mut t = s.trim().to_string();
    for br in ["<br/>", "<br />", "<br>", "<BR>", "<BR/>"] {
        t = t.replace(br, "\n");
    }
    t.replace("&quot;", "\"").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
}

fn take_run(c: &mut Cur, set: &str) -> String {
    let mut r = String::new();
    while let Some(ch) = c.peek() {
        if set.contains(ch) {
            r.push(ch);
            c.i += 1;
        } else {
            break;
        }
    }
    r
}

fn eat_head(c: &mut Cur) -> bool {
    if c.eat(">") {
        return true;
    }
    if matches!(c.peek(), Some('x' | 'o')) && c.at(1).is_none_or(|n| n.is_whitespace() || n == '|') {
        c.i += 1;
        return true;
    }
    false
}

fn parse_link(c: &mut Cur) -> Option<Link> {
    let start = c.i;
    let tail = c.eat("<");
    let run1 = take_run(c, "-=.~");
    if run1.len() < 2 && !(run1.len() == 1 && c.peek() == Some('>')) {
        c.i = start;
        return None;
    }
    let mut head = eat_head(c);
    let mut all = run1.clone();
    let mut label = None;

    // Text form: `A -- text --> B`.
    if !head && !tail && matches!(run1.as_str(), "--" | "==" | "-.") && c.peek() == Some(' ') {
        let save = c.i;
        let mut text = String::new();
        let mut found = false;
        while !c.eof() {
            let here = c.i;
            if c.peek() == Some(' ') || text.is_empty() {
                let probe = c.i + usize::from(c.peek() == Some(' '));
                c.i = probe;
                let run2 = take_run(c, "-=.");
                if run2.len() >= 2 {
                    head = eat_head(c);
                    all.push_str(&run2);
                    found = true;
                    break;
                }
                c.i = here;
            }
            text.push(c.peek().unwrap_or(' '));
            c.i += 1;
        }
        if found {
            label = Some(clean_label(&text));
        } else {
            c.i = save;
        }
    }
    let stroke = if all.contains('=') {
        Stroke::Heavy
    } else if all.contains('.') {
        Stroke::Dotted
    } else {
        Stroke::Light
    };
    c.ws();
    if c.peek() == Some('|') {
        c.i += 1;
        let mut text = String::new();
        while let Some(ch) = c.peek() {
            c.i += 1;
            if ch == '|' {
                break;
            }
            text.push(ch);
        }
        label = Some(clean_label(text.trim_matches('"')));
    }
    Some(Link { stroke, head, tail, label })
}

// ---------------------------------------------------------------------------
// State diagram
// ---------------------------------------------------------------------------

fn parse_state(body: &[&str]) -> Result<Graph, String> {
    let mut g = Graph::new(Dir::TopDown);
    let node = |g: &mut Graph, name: &str, source: bool| -> usize {
        let name = name.trim();
        if name == "[*]" {
            let (id, label) = if source { ("[*]start", "●") } else { ("[*]end", "◉") };
            return g.set_node(id, Some(label.into()), Some(Shape::Round));
        }
        let i = g.node(name);
        g.nodes[i].shape = if g.nodes[i].shape == Shape::Diamond { Shape::Diamond } else { Shape::Round };
        i
    };
    let mut in_note = false;
    for &line in body {
        let lower = line.to_ascii_lowercase();
        if in_note {
            in_note = !lower.starts_with("end note");
            continue;
        }
        if lower.starts_with("note ") {
            in_note = !line.contains(':');
            continue;
        }
        if let Some(d) = lower.strip_prefix("direction ") {
            g.dir = parse_dir(d.trim());
            continue;
        }
        if line == "}" || line == "--" || lower.starts_with("classdef") || lower.starts_with("class ") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("state ") {
            let rest = rest.trim_end_matches('{').trim();
            if let Some((label, id)) = rest.split_once(" as ") {
                g.set_node(id.trim(), Some(label.trim().trim_matches('"').to_string()), Some(Shape::Round));
            } else if let Some(id) = rest.strip_suffix("<<choice>>") {
                g.set_node(id.trim(), None, Some(Shape::Diamond));
            } else {
                let id = rest.split_whitespace().next().unwrap_or(rest);
                node(&mut g, id, true);
            }
            continue;
        }
        if let Some((lhs, rhs)) = line.split_once("-->") {
            let (rhs, label) = match rhs.split_once(':') {
                Some((r, l)) => (r, Some(l.trim().to_string())),
                None => (rhs, None),
            };
            let a = node(&mut g, lhs, true);
            let b = node(&mut g, rhs, false);
            g.edges.push(Edge { from: a, to: b, label, stroke: Stroke::Light, head: true, tail: false });
        } else if let Some((id, desc)) = line.split_once(':') {
            let i = node(&mut g, id, true);
            g.nodes[i].label = format!("{}\n{}", g.nodes[i].id, desc.trim());
        } else {
            node(&mut g, line, true);
        }
    }
    if g.nodes.is_empty() {
        return Err("state diagram has no states".into());
    }
    Ok(g)
}

// ---------------------------------------------------------------------------
// Charts
// ---------------------------------------------------------------------------

fn parse_pie(header: &str, body: &[&str]) -> Result<Chart, String> {
    let mut chart = Chart { kind: Kind::Bar, title: None, labels: Vec::new(), series: Vec::new(), percent: true };
    let mut values = Vec::new();
    if let Some(t) = header.split_once("title") {
        chart.title = Some(t.1.trim().to_string());
    }
    for &line in body {
        if let Some(t) = line.strip_prefix("title") {
            chart.title = Some(t.trim().to_string());
            continue;
        }
        let Some((label, value)) = line.rsplit_once(':') else {
            continue;
        };
        let v: f64 = value.trim().parse().map_err(|_| format!("\"{}\" is not a number", value.trim()))?;
        chart.labels.push(label.trim().trim_matches('"').to_string());
        values.push(v);
    }
    chart.series.push(Series { name: None, values });
    Ok(chart)
}

fn bracket_list(s: &str) -> Option<Vec<String>> {
    let inner = s.split_once('[')?.1.rsplit_once(']')?.0;
    Some(inner.split(',').map(|v| v.trim().trim_matches('"').to_string()).filter(|v| !v.is_empty()).collect())
}

fn parse_xychart(header: &str, body: &[&str]) -> Result<Chart, String> {
    let horizontal = header.contains("horizontal");
    let mut chart = Chart { kind: Kind::Column, title: None, labels: Vec::new(), series: Vec::new(), percent: false };
    let mut has_line = false;
    for &line in body {
        let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        match word {
            "title" => chart.title = Some(rest.trim().trim_matches('"').to_string()),
            "x-axis" => {
                if let Some(l) = bracket_list(rest) {
                    chart.labels = l;
                }
            }
            "bar" | "line" => {
                let vals = bracket_list(rest).ok_or_else(|| format!("cannot read \"{line}\""))?;
                let values = vals
                    .iter()
                    .map(|v| v.parse::<f64>().map_err(|_| format!("\"{v}\" is not a number")))
                    .collect::<Result<Vec<f64>, String>>()?;
                has_line |= word == "line";
                chart.series.push(Series { name: Some(word.to_string()), values });
            }
            _ => {}
        }
    }
    let n = chart.series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    while chart.labels.len() < n {
        chart.labels.push((chart.labels.len() + 1).to_string());
    }
    if chart.series.len() == 1 {
        chart.series[0].name = None;
    }
    chart.kind = if has_line {
        Kind::Line
    } else if horizontal {
        Kind::Bar
    } else {
        Kind::Column
    };
    Ok(chart)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(src: &str) -> Graph {
        let lines: Vec<&str> = src.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        parse_flowchart(lines[0], &lines[1..]).unwrap()
    }

    fn edges(g: &Graph) -> Vec<(String, String, Option<String>)> {
        g.edges.iter().map(|e| (g.nodes[e.from].id.clone(), g.nodes[e.to].id.clone(), e.label.clone())).collect()
    }

    #[test]
    fn flowchart_shapes_and_labels() {
        let g = flow("graph LR\nA[Start] --> B{Ok?}\nB -->|yes| C(Done)\nB -- no --> A");
        assert_eq!(g.dir, Dir::LeftRight);
        assert_eq!(g.nodes[0].label, "Start");
        assert_eq!(g.nodes[1].shape, Shape::Diamond);
        assert_eq!(g.nodes[2].shape, Shape::Round);
        assert_eq!(
            edges(&g),
            vec![
                ("A".into(), "B".into(), None),
                ("B".into(), "C".into(), Some("yes".into())),
                ("B".into(), "A".into(), Some("no".into())),
            ]
        );
    }

    #[test]
    fn flowchart_chains_groups_and_strokes() {
        let g = flow("flowchart TD\nA & B --> C ==> D -.-> E --- F");
        assert_eq!(g.edges.len(), 5);
        assert_eq!(g.edges[2].stroke, Stroke::Heavy);
        assert_eq!(g.edges[3].stroke, Stroke::Dotted);
        assert!(!g.edges[4].head);
    }

    #[test]
    fn flowchart_quoted_label_and_semicolons() {
        let g = flow("graph TD; A[\"a [b] c\"] --> B;");
        assert_eq!(g.nodes[0].label, "a [b] c");
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn state_diagram_start_end() {
        let body = ["[*] --> Idle", "Idle --> Busy : start", "Busy --> [*]"];
        let g = parse_state(&body).unwrap();
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.edges[1].label.as_deref(), Some("start"));
    }

    #[test]
    fn pie_and_xychart() {
        let p = parse_pie("pie title Pets", &["\"Dogs\" : 3", "\"Cats\" : 1"]).unwrap();
        assert_eq!(p.title.as_deref(), Some("Pets"));
        assert_eq!(p.series[0].values, vec![3.0, 1.0]);
        let x = parse_xychart("xychart-beta", &["x-axis [a, b, c]", "line [1, 2, 3]"]).unwrap();
        assert_eq!(x.kind, Kind::Line);
        assert_eq!(x.labels.len(), 3);
    }
}
