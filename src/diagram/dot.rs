//! Parser for the common subset of the Graphviz DOT language.

use super::canvas::Stroke;
use super::graph::{Dir, Edge, Graph, Shape};

#[derive(Clone, PartialEq, Debug)]
enum Tok {
    Id(String),
    Sym(char),
    Arrow,
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    let mut line_start = true;
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        let at_line_start = line_start;
        line_start = false;
        if (ch == '/' && c.get(i + 1) == Some(&'/')) || (ch == '#' && at_line_start) {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if ch == '"' {
            i += 1;
            let mut s = String::new();
            while i < c.len() && c[i] != '"' {
                if c[i] == '\\' && i + 1 < c.len() {
                    match c[i + 1] {
                        'n' | 'l' | 'r' => s.push('\n'),
                        other => s.push(other),
                    }
                    i += 2;
                    continue;
                }
                s.push(c[i]);
                i += 1;
            }
            i += 1;
            out.push(Tok::Id(s));
            continue;
        }
        if ch == '<' {
            // HTML-like label: keep the text, drop the tags.
            let mut depth = 0;
            let mut s = String::new();
            let mut in_tag = false;
            while i < c.len() {
                match c[i] {
                    '<' => {
                        depth += 1;
                        in_tag = depth > 1;
                    }
                    '>' => {
                        depth -= 1;
                        in_tag = false;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    ch if !in_tag && depth >= 1 => s.push(ch),
                    _ => {}
                }
                i += 1;
            }
            out.push(Tok::Id(s.trim().to_string()));
            continue;
        }
        if ch == '-' && matches!(c.get(i + 1), Some('>') | Some('-')) {
            out.push(Tok::Arrow);
            i += 2;
            continue;
        }
        if ch.is_alphanumeric() || ch == '_' || ch == '.' || ch == '-' {
            let mut s = String::new();
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '.') {
                s.push(c[i]);
                i += 1;
            }
            if s.is_empty() {
                s.push(ch);
                i += 1;
            }
            out.push(Tok::Id(s));
            continue;
        }
        if "{}[]=;,:+".contains(ch) {
            out.push(Tok::Sym(ch));
            i += 1;
            continue;
        }
        return Err(format!("unexpected character '{ch}' in dot source"));
    }
    Ok(out)
}

const MAX_DEPTH: usize = 32;

struct P {
    t: Vec<Tok>,
    i: usize,
    depth: usize,
    g: Graph,
    directed: bool,
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn peek_at(&self, k: usize) -> Option<&Tok> {
        self.t.get(self.i + k)
    }
    fn eat_sym(&mut self, c: char) -> bool {
        if self.peek() == Some(&Tok::Sym(c)) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expect_sym(&mut self, c: char) -> Result<(), String> {
        if self.eat_sym(c) { Ok(()) } else { Err(format!("expected '{c}' in dot source")) }
    }
    fn id(&mut self) -> Result<String, String> {
        match self.peek().cloned() {
            Some(Tok::Id(s)) => {
                self.i += 1;
                Ok(s)
            }
            other => Err(format!("expected a name in dot source, found {other:?}")),
        }
    }

    fn node(&mut self, id: &str) -> usize {
        let before = self.g.nodes.len();
        let i = self.g.node(id);
        if self.g.nodes.len() > before {
            // Graphviz draws ellipses by default.
            self.g.nodes[i].shape = Shape::Round;
        }
        i
    }

    fn attrs(&mut self) -> Result<Vec<(String, String)>, String> {
        let mut out = Vec::new();
        while self.eat_sym('[') {
            while !self.eat_sym(']') {
                let k = self.id()?;
                let v = if self.eat_sym('=') { self.id()? } else { "true".into() };
                out.push((k.to_ascii_lowercase(), v));
                let _ = self.eat_sym(',') || self.eat_sym(';');
                if self.peek().is_none() {
                    return Err("unclosed '[' in dot source".into());
                }
            }
        }
        Ok(out)
    }

    fn set_rankdir(&mut self, v: &str) {
        self.g.dir = match v.to_ascii_uppercase().as_str() {
            "LR" => Dir::LeftRight,
            "RL" => Dir::RightLeft,
            "BT" => Dir::BottomUp,
            _ => Dir::TopDown,
        };
    }

    fn stmts(&mut self) -> Result<Vec<usize>, String> {
        let mut all = Vec::new();
        loop {
            match self.peek() {
                None => return Err("missing '}' in dot source".into()),
                Some(Tok::Sym('}')) => {
                    self.i += 1;
                    return Ok(all);
                }
                Some(Tok::Sym(';')) | Some(Tok::Sym(',')) => self.i += 1,
                _ => all.extend(self.stmt()?),
            }
        }
    }

    fn subgraph(&mut self) -> Result<Vec<usize>, String> {
        if matches!(self.peek(), Some(Tok::Id(k)) if k.eq_ignore_ascii_case("subgraph")) {
            self.i += 1;
            if matches!(self.peek(), Some(Tok::Id(_))) {
                self.i += 1;
            }
        }
        self.expect_sym('{')?;
        if self.depth >= MAX_DEPTH {
            return Err("subgraphs nested too deeply".into());
        }
        self.depth += 1;
        let r = self.stmts();
        self.depth -= 1;
        r
    }

    fn operand(&mut self) -> Result<Vec<usize>, String> {
        let is_sub = matches!(self.peek(), Some(Tok::Sym('{')))
            || matches!(self.peek(), Some(Tok::Id(k)) if k.eq_ignore_ascii_case("subgraph"));
        if is_sub {
            return self.subgraph();
        }
        let id = self.id()?;
        // Ports like `a:n` or `a:p:n` are ignored.
        while self.eat_sym(':') {
            self.id()?;
        }
        Ok(vec![self.node(&id)])
    }

    fn stmt(&mut self) -> Result<Vec<usize>, String> {
        if let Some(Tok::Id(k)) = self.peek().cloned() {
            let kl = k.to_ascii_lowercase();
            if matches!(kl.as_str(), "graph" | "node" | "edge") && self.peek_at(1) == Some(&Tok::Sym('[')) {
                self.i += 1;
                let attrs = self.attrs()?;
                if kl == "graph"
                    && let Some((_, v)) = attrs.iter().find(|(k, _)| k == "rankdir")
                {
                    let v = v.clone();
                    self.set_rankdir(&v);
                }
                return Ok(Vec::new());
            }
            if self.peek_at(1) == Some(&Tok::Sym('=')) {
                self.i += 2;
                let v = self.id()?;
                if kl == "rankdir" {
                    self.set_rankdir(&v);
                }
                return Ok(Vec::new());
            }
        }
        let first = self.operand()?;
        let mut chain = vec![first.clone()];
        while self.peek() == Some(&Tok::Arrow) {
            self.i += 1;
            chain.push(self.operand()?);
        }
        let attrs = self.attrs()?;
        let get = |k: &str| attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
        if chain.len() == 1 {
            if first.len() == 1 {
                let i = first[0];
                if let Some(l) = get("label") {
                    self.g.nodes[i].label = l;
                }
                if let Some(s) = get("shape") {
                    self.g.nodes[i].shape = match s.as_str() {
                        "box" | "rect" | "rectangle" | "square" | "record" | "plaintext" | "plain" | "none" | "note" | "tab" | "folder" | "component" => Shape::Rect,
                        "diamond" | "Mdiamond" | "hexagon" | "octagon" => Shape::Diamond,
                        _ => Shape::Round,
                    };
                }
            }
            return Ok(first);
        }
        let stroke = match get("style").as_deref() {
            Some("dashed") | Some("dotted") => Stroke::Dotted,
            Some("bold") => Stroke::Heavy,
            _ => Stroke::Light,
        };
        let (head, tail) = match get("dir").as_deref() {
            Some("both") => (true, true),
            Some("back") => (false, true),
            Some("none") => (false, false),
            _ => (self.directed, false),
        };
        let label = get("label").or_else(|| get("xlabel"));
        let mut all = Vec::new();
        for pair in chain.windows(2) {
            for &a in &pair[0] {
                for &b in &pair[1] {
                    if self.g.edges.len() >= super::graph::MAX_EDGES {
                        return Err("graph too large".into());
                    }
                    self.g.edges.push(Edge { from: a, to: b, label: label.clone(), stroke, head, tail });
                }
            }
        }
        for group in chain {
            all.extend(group);
        }
        Ok(all)
    }
}

pub fn parse(src: &str) -> Result<Graph, String> {
    let t = lex(src)?;
    let mut p = P { t, i: 0, depth: 0, g: Graph::new(Dir::TopDown), directed: true };
    if matches!(p.peek(), Some(Tok::Id(k)) if k.eq_ignore_ascii_case("strict")) {
        p.i += 1;
    }
    match p.id()?.to_ascii_lowercase().as_str() {
        "digraph" => p.directed = true,
        "graph" => p.directed = false,
        other => return Err(format!("expected 'graph' or 'digraph', found '{other}'")),
    }
    if matches!(p.peek(), Some(Tok::Id(_))) {
        p.i += 1;
    }
    p.expect_sym('{')?;
    p.stmts()?;
    if p.g.nodes.is_empty() {
        return Err("graph has no nodes".into());
    }
    Ok(p.g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_digraph_with_attrs() {
        let g = parse(
            r#"digraph G {
                rankdir=LR; // comment
                node [shape=box];
                a [label="Start\nhere"];
                a -> b -> c [label="go", style=dashed];
                b -> {d e}
            }"#,
        )
        .unwrap();
        assert_eq!(g.dir, Dir::LeftRight);
        assert_eq!(g.nodes[0].label, "Start\nhere");
        assert_eq!(g.edges.len(), 4);
        assert_eq!(g.edges[0].stroke, Stroke::Dotted);
        assert_eq!(g.edges[1].label.as_deref(), Some("go"));
    }

    #[test]
    fn undirected_graph_has_no_heads() {
        let g = parse("graph { a -- b }").unwrap();
        assert!(!g.edges[0].head);
    }

    #[test]
    fn deep_nesting_is_an_error() {
        let src = format!("digraph {{{}", "{".repeat(100_000));
        assert!(parse(&src).is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("hello").is_err());
    }
}
