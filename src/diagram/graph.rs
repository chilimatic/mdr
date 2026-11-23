//! Layered graph layout drawn as text: boxes for nodes, routed lines for edges.
//!
//! Steps: break cycles, assign layers (longest path), add dummy nodes for long
//! edges, order each layer by barycenter, place nodes, then route each edge
//! through the gap between two layers on its own track.

use std::collections::HashMap;

use unicode_width::UnicodeWidthStr;

use super::Palette;
use super::canvas::{Canvas, Stroke};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    TopDown,
    BottomUp,
    LeftRight,
    RightLeft,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Rect,
    Round,
    Diamond,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Option<String>,
    pub stroke: Stroke,
    /// Arrow head at `to`.
    pub head: bool,
    /// Arrow head at `from`.
    pub tail: bool,
}

#[derive(Debug)]
pub struct Graph {
    pub dir: Dir,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    index: HashMap<String, usize>,
}

impl Graph {
    pub fn new(dir: Dir) -> Self {
        Graph { dir, nodes: Vec::new(), edges: Vec::new(), index: HashMap::new() }
    }

    /// Returns the node with this id. Creates it, labelled with its id, if it does not exist.
    pub fn node(&mut self, id: &str) -> usize {
        if let Some(&i) = self.index.get(id) {
            return i;
        }
        self.nodes.push(Node { id: id.to_string(), label: id.to_string(), shape: Shape::Rect });
        self.index.insert(id.to_string(), self.nodes.len() - 1);
        self.nodes.len() - 1
    }

    pub fn set_node(&mut self, id: &str, label: Option<String>, shape: Option<Shape>) -> usize {
        let i = self.node(id);
        if let Some(l) = label {
            self.nodes[i].label = l;
        }
        if let Some(s) = shape {
            self.nodes[i].shape = s;
        }
        i
    }
}

// ---------------------------------------------------------------------------

struct VNode {
    /// Index in `Graph::nodes`, or `None` for a dummy node on a long edge.
    real: Option<usize>,
    layer: usize,
    /// Size along the layer axis (height for top-down) and across it.
    main: usize,
    cross: usize,
    lines: Vec<String>,
}

struct Seg {
    upper: usize,
    lower: usize,
    edge: usize,
    first: bool,
    last: bool,
}

const NODE_GAP: i64 = 2;
pub const MAX_NODES: usize = 300;
pub const MAX_EDGES: usize = 1000;
/// Limits for dummy nodes and canvas height, so a large input cannot freeze the viewer.
const MAX_DUMMIES: usize = 5000;
const MAX_HEIGHT: usize = 3000;

pub fn render(g: &Graph, max_width: usize, pal: &Palette) -> Result<Canvas, String> {
    if g.nodes.is_empty() {
        return Err("diagram has no nodes".into());
    }
    if g.nodes.len() > MAX_NODES || g.edges.len() > MAX_EDGES {
        return Err(format!("diagram too large (at most {MAX_NODES} nodes and {MAX_EDGES} edges)"));
    }
    let vertical = matches!(g.dir, Dir::TopDown | Dir::BottomUp);
    let n = g.nodes.len();

    // 1. Break cycles: a DFS back edge is laid out reversed.
    let edges: Vec<usize> = (0..g.edges.len()).filter(|&e| g.edges[e].from != g.edges[e].to).collect();
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &e in &edges {
        out[g.edges[e].from].push(e);
    }
    let mut reversed = vec![false; g.edges.len()];
    let mut state = vec![0u8; n];
    // Iterative DFS: (node, index of the next out-edge to visit).
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        state[root] = 1;
        while let Some(top) = stack.last_mut() {
            let (v, k) = *top;
            if k == out[v].len() {
                state[v] = 2;
                stack.pop();
                continue;
            }
            top.1 += 1;
            let e = out[v][k];
            let w = g.edges[e].to;
            match state[w] {
                0 => {
                    state[w] = 1;
                    stack.push((w, 0));
                }
                1 => reversed[e] = true,
                _ => {}
            }
        }
    }
    let ends = |e: usize| -> (usize, usize) {
        let ed = &g.edges[e];
        if reversed[e] { (ed.to, ed.from) } else { (ed.from, ed.to) }
    };

    // 2. Layers by longest path from the sources.
    let mut indeg = vec![0usize; n];
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &e in &edges {
        let (a, b) = ends(e);
        succ[a].push(b);
        indeg[b] += 1;
    }
    let mut layer = vec![0usize; n];
    let mut queue: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut qi = 0;
    while qi < queue.len() {
        let v = queue[qi];
        qi += 1;
        for &w in &succ[v] {
            layer[w] = layer[w].max(layer[v] + 1);
            indeg[w] -= 1;
            if indeg[w] == 0 {
                queue.push(w);
            }
        }
    }

    // 3. Virtual nodes: real nodes first, then dummies for edges that skip layers.
    let mut vn: Vec<VNode> = g
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let lines: Vec<String> = node.label.lines().map(|l| l.trim().to_string()).collect();
            let lines = if lines.is_empty() { vec![String::new()] } else { lines };
            let w = lines.iter().map(|l| l.width()).max().unwrap_or(0) + 4;
            let h = lines.len() + 2;
            let (main, cross) = if vertical { (h, w) } else { (w, h) };
            VNode { real: Some(i), layer: layer[i], main, cross, lines }
        })
        .collect();
    let mut segs: Vec<Seg> = Vec::new();
    for &e in &edges {
        let (a, b) = ends(e);
        let mut prev = a;
        for l in layer[a] + 1..layer[b] {
            vn.push(VNode { real: None, layer: l, main: 0, cross: 1, lines: Vec::new() });
            let d = vn.len() - 1;
            segs.push(Seg { upper: prev, lower: d, edge: e, first: prev == a, last: false });
            prev = d;
        }
        segs.push(Seg { upper: prev, lower: b, edge: e, first: prev == a, last: true });
        if vn.len() - n > MAX_DUMMIES {
            return Err("diagram too large".into());
        }
    }
    let nlayers = vn.iter().map(|v| v.layer).max().unwrap_or(0) + 1;
    let mut ups: Vec<Vec<usize>> = vec![Vec::new(); vn.len()];
    let mut downs: Vec<Vec<usize>> = vec![Vec::new(); vn.len()];
    for s in &segs {
        downs[s.upper].push(s.lower);
        ups[s.lower].push(s.upper);
    }

    // 4. Order within layers: barycenter sweeps.
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); nlayers];
    for (i, v) in vn.iter().enumerate() {
        layers[v.layer].push(i);
    }
    let mut order = vec![0usize; vn.len()];
    let reindex = |layers: &Vec<Vec<usize>>, order: &mut Vec<usize>| {
        for l in layers {
            for (k, &v) in l.iter().enumerate() {
                order[v] = k;
            }
        }
    };
    reindex(&layers, &mut order);
    for _ in 0..6 {
        for l in 1..nlayers {
            sort_by_bary(&mut layers[l], &ups, &order);
            reindex(&layers, &mut order);
        }
        for l in (0..nlayers.saturating_sub(1)).rev() {
            sort_by_bary(&mut layers[l], &downs, &order);
            reindex(&layers, &mut order);
        }
    }

    // 5. Positions across the layer axis: pull nodes toward their neighbors' centers.
    let mut pos = vec![0i64; vn.len()];
    for l in &layers {
        let mut x = 0;
        for &v in l {
            pos[v] = x;
            x += vn[v].cross as i64 + NODE_GAP;
        }
    }
    let center = |pos: &[i64], v: usize| pos[v] + vn[v].cross as i64 / 2;
    let place = |pos: &mut Vec<i64>, l: usize, nbrs: &[Vec<usize>]| {
        let mut prev_end = i64::MIN / 2;
        for &v in &layers[l] {
            let want = if nbrs[v].is_empty() {
                pos[v]
            } else {
                let avg = nbrs[v].iter().map(|&u| center(pos, u)).sum::<i64>() / nbrs[v].len() as i64;
                avg - vn[v].cross as i64 / 2
            };
            let gap = if vn[v].real.is_none() { 1 } else { NODE_GAP };
            pos[v] = want.max(prev_end + gap);
            prev_end = pos[v] + vn[v].cross as i64;
        }
    };
    for _ in 0..4 {
        for l in 1..nlayers {
            place(&mut pos, l, &ups);
        }
        for l in (0..nlayers.saturating_sub(1)).rev() {
            place(&mut pos, l, &downs);
        }
    }
    let min = pos.iter().copied().min().unwrap_or(0);
    for p in &mut pos {
        *p -= min;
    }
    let cross_total = (0..vn.len()).map(|v| pos[v] as usize + vn[v].cross).max().unwrap_or(1);

    // 6. Gaps between layers: a stub, one track per horizontal run, a label slot and the arrow.
    let mut layer_main = vec![1usize; nlayers];
    for v in &vn {
        layer_main[v.layer] = layer_main[v.layer].max(v.main);
    }
    // Attach points. A node with forward and back edges on one side gets the back
    // edges off-center, so the two directions do not share one line.
    let mut fwd_down = vec![false; vn.len()];
    let mut rev_down = vec![false; vn.len()];
    let mut fwd_up = vec![false; vn.len()];
    let mut rev_up = vec![false; vn.len()];
    for s in &segs {
        if reversed[s.edge] {
            rev_down[s.upper] = true;
            rev_up[s.lower] = true;
        } else {
            fwd_down[s.upper] = true;
            fwd_up[s.lower] = true;
        }
    }
    let attach = |v: usize, rev: bool, mixed: bool| -> i64 {
        if rev && mixed && vn[v].real.is_some() {
            let off = if vn[v].cross >= 5 { vn[v].cross - 2 } else { vn[v].cross - 1 };
            pos[v] + off as i64
        } else {
            center(&pos, v)
        }
    };
    let ends_x: Vec<(i64, i64)> = segs
        .iter()
        .map(|s| {
            let r = reversed[s.edge];
            (attach(s.upper, r, fwd_down[s.upper] && rev_down[s.upper]), attach(s.lower, r, fwd_up[s.lower] && rev_up[s.lower]))
        })
        .collect();

    struct Gap {
        tracks: HashMap<(usize, i64), usize>,
        ntracks: usize,
        /// Size of the label area, and the label row of each labelled segment.
        label: usize,
        label_row: HashMap<usize, usize>,
    }
    let mut gaps: Vec<Gap> = Vec::new();
    let mut label_extent = 0usize;
    for l in 0..nlayers.saturating_sub(1) {
        // Group the gap's segments by source point; each group needs one track if it bends.
        let mut groups: Vec<((usize, i64), i64, i64)> = Vec::new();
        let mut label = 0usize;
        let mut label_row = HashMap::new();
        let mut rows: Vec<Vec<(i64, i64)>> = Vec::new();
        for (si, s) in segs.iter().enumerate().filter(|(_, s)| vn[s.upper].layer == l) {
            let (sx, tx) = ends_x[si];
            // Labels go on the first segment, next to the source (like "yes"/"no" under a decision).
            if s.first
                && let Some(lb) = &g.edges[s.edge].label
            {
                if vertical {
                    // Labels sit centered on their line; a label that would overlap another moves down a row.
                    let a = (tx - lb.width() as i64 / 2).max(0);
                    let b = a + lb.width() as i64;
                    label_extent = label_extent.max(b as usize);
                    let k = rows
                        .iter()
                        .position(|r| r.iter().all(|&(x0, x1)| b < x0 || a > x1))
                        .unwrap_or_else(|| {
                            rows.push(Vec::new());
                            rows.len() - 1
                        });
                    rows[k].push((a, b));
                    label_row.insert(si, k);
                    label = rows.len();
                } else {
                    label = label.max(lb.width() + 2);
                    label_row.insert(si, 0);
                }
            }
            if sx == tx {
                continue;
            }
            let (a, b) = (sx.min(tx), sx.max(tx));
            match groups.iter_mut().find(|gr| gr.0 == (s.upper, sx)) {
                Some(gr) => {
                    gr.1 = gr.1.min(a);
                    gr.2 = gr.2.max(b);
                }
                None => groups.push(((s.upper, sx), a, b)),
            }
        }
        groups.sort_by_key(|g| g.1);
        let mut track_end: Vec<i64> = Vec::new();
        let mut tracks = HashMap::new();
        for (src, a, b) in groups {
            let t = match track_end.iter().position(|&end| end + 1 < a) {
                Some(t) => t,
                None => {
                    track_end.push(i64::MIN);
                    track_end.len() - 1
                }
            };
            track_end[t] = b;
            tracks.insert(src, t);
        }
        gaps.push(Gap { ntracks: track_end.len(), tracks, label, label_row });
    }
    let cross_total = cross_total.max(label_extent);
    let gap_size = |gp: &Gap| 1 + gp.ntracks + gp.label + 1;
    let mut layer_start = vec![0usize; nlayers];
    let mut m = 0;
    for l in 0..nlayers {
        layer_start[l] = m;
        m += layer_main[l];
        if l + 1 < nlayers {
            m += gap_size(&gaps[l]);
        }
    }
    let main_total = m;

    let (w, h) = if vertical { (cross_total, main_total) } else { (main_total, cross_total) };
    if w > max_width {
        return Err(format!("diagram needs {w} columns, {max_width} available"));
    }
    if h > MAX_HEIGHT {
        return Err(format!("diagram too tall ({h} rows)"));
    }
    let flip = matches!(g.dir, Dir::BottomUp | Dir::RightLeft);
    let fm = |main: usize| if flip { main_total - 1 - main } else { main };
    let xy = |main: usize, cross: usize| if vertical { (cross, fm(main)) } else { (fm(main), cross) };

    let mut c = Canvas::new(w, h);

    // Nodes.
    for (i, v) in vn.iter().enumerate() {
        let l = v.layer;
        let cross0 = pos[i] as usize;
        if v.real.is_none() {
            continue;
        }
        let main0 = layer_start[l];
        let (bw, bh) = if vertical { (v.cross, v.main) } else { (v.main, v.cross) };
        let (x, y) = if vertical { (cross0, fm(main0).min(fm(main0 + v.main - 1))) } else { (fm(main0).min(fm(main0 + v.main - 1)), cross0) };
        let shape = g.nodes[v.real.unwrap()].shape;
        let corners = match shape {
            Shape::Rect => ['┌', '┐', '└', '┘'],
            Shape::Round => ['╭', '╮', '╰', '╯'],
            Shape::Diamond => ['╱', '╲', '╲', '╱'],
        };
        c.frame(x, y, bw, bh, corners, pal.border);
        for (k, line) in v.lines.iter().enumerate() {
            let lw = line.width();
            c.text(x + (bw - lw) / 2, y + 1 + k, line, pal.text);
        }
    }

    // Dummy nodes: a straight line through their layer.
    for (i, v) in vn.iter().enumerate() {
        if v.real.is_some() {
            continue;
        }
        let e = segs.iter().find(|s| s.lower == i).map(|s| s.edge).unwrap();
        let cx = pos[i] as usize;
        let (a, b) = (layer_start[v.layer], layer_start[v.layer] + layer_main[v.layer] - 1);
        let (x0, y0) = xy(a, cx);
        let (x1, y1) = xy(b, cx);
        c.line(x0, y0, x1, y1, g.edges[e].stroke, pal.edge);
    }

    // Edges, one segment per gap.
    let fwd_arrow = match g.dir {
        Dir::TopDown => '▼',
        Dir::BottomUp => '▲',
        Dir::LeftRight => '▶',
        Dir::RightLeft => '◀',
    };
    let back_arrow = match g.dir {
        Dir::TopDown => '▲',
        Dir::BottomUp => '▼',
        Dir::LeftRight => '◀',
        Dir::RightLeft => '▶',
    };
    let mut labels: Vec<(usize, usize, String)> = Vec::new();
    for (si, s) in segs.iter().enumerate() {
        let l = vn[s.upper].layer;
        let gp = &gaps[l];
        let edge = &g.edges[s.edge];
        let (sx, tx) = (ends_x[si].0 as usize, ends_x[si].1 as usize);
        // Nodes sit at the start of their layer, so a shorter source node gets a longer stub.
        let g0 = layer_start[l] + layer_main[l];
        let start = if vn[s.upper].real.is_some() { layer_start[l] + vn[s.upper].main } else { g0 };
        let stop = layer_start[l + 1] - 1; // the cell next to the lower node
        let line = |c: &mut Canvas, m0: usize, x0: usize, m1: usize, x1: usize| {
            let (ax, ay) = xy(m0, x0);
            let (bx, by) = xy(m1, x1);
            c.line(ax, ay, bx, by, edge.stroke, pal.edge);
        };
        if sx == tx {
            line(&mut c, start, sx, stop, sx);
        } else {
            let t = g0 + 1 + gp.tracks[&(s.upper, sx as i64)];
            line(&mut c, start, sx, t, sx);
            line(&mut c, t, sx, t, tx);
            line(&mut c, t, tx, stop, tx);
        }
        let head_low = (edge.head && !reversed[s.edge]) || (edge.tail && reversed[s.edge]);
        let head_up = (edge.head && reversed[s.edge]) || (edge.tail && !reversed[s.edge]);
        if s.last && head_low {
            let (x, y) = xy(stop, tx);
            c.put(x, y, fwd_arrow, pal.arrow);
        }
        if s.first && head_up {
            let (x, y) = xy(start, sx);
            c.put(x, y, back_arrow, pal.arrow);
        }
        if s.first
            && let Some(lb) = &edge.label
        {
            let lw = lb.width();
            let row = gp.label_row.get(&si).copied().unwrap_or(0);
            if vertical {
                let (_, y) = xy(stop - gp.label + row, tx);
                labels.push((tx.saturating_sub(lw / 2), y, lb.clone()));
            } else {
                let m0 = stop - gp.label;
                let (x0, y) = xy(m0 + 1, tx);
                let (x1, _) = xy(m0 + lw, tx);
                labels.push((x0.min(x1), y, lb.clone()));
            }
        }
    }
    for (x, y, lb) in labels {
        c.text(x, y, &lb, pal.label);
    }
    Ok(c)
}

fn sort_by_bary(layer: &mut [usize], nbrs: &[Vec<usize>], order: &[usize]) {
    let key = |v: usize| -> f64 {
        if nbrs[v].is_empty() {
            order[v] as f64
        } else {
            nbrs[v].iter().map(|&u| order[u] as f64).sum::<f64>() / nbrs[v].len() as f64
        }
    };
    let mut keyed: Vec<(f64, usize, usize)> = layer.iter().map(|&v| (key(v), order[v], v)).collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    for (slot, (_, _, v)) in layer.iter_mut().zip(keyed) {
        *slot = v;
    }
}
