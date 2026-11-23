//! Text-art diagrams for fenced code blocks: ```mermaid, ```dot / ```graphviz and ```chart.

pub mod canvas;
mod chart;
mod dot;
mod graph;
mod mermaid;
mod sequence;

use ratatui::style::{Modifier, Style};

use crate::theme::Theme;

/// Graphs and sequence diagrams wider than the window are drawn anyway (the
/// viewer scrolls sideways), up to this many columns.
pub const MAX_DIAGRAM_WIDTH: usize = 400;

/// Rendered rows of (text, style) pieces.
pub type Lines = Vec<Vec<(String, Style)>>;

pub struct Palette {
    pub border: Style,
    pub text: Style,
    pub edge: Style,
    pub arrow: Style,
    pub label: Style,
    pub title: Style,
    pub dim: Style,
    series: [Style; 5],
}

impl Palette {
    pub fn new(t: &Theme) -> Self {
        let fg = |s: Style| Style { fg: s.fg, ..Style::default() };
        Palette {
            border: fg(t.headings[1]),
            text: t.text,
            edge: fg(t.rule),
            arrow: fg(t.headings[1]),
            label: t.dim.add_modifier(Modifier::ITALIC),
            title: t.strong,
            dim: t.dim,
            series: [fg(t.headings[1]), fg(t.headings[4]), fg(t.headings[3]), fg(t.headings[0]), fg(t.headings[2])],
        }
    }

    pub fn series(&self, i: usize) -> Style {
        self.series[i % self.series.len()]
    }
}

/// Code block languages drawn as diagrams instead of code.
pub fn is_diagram(lang: &str) -> bool {
    matches!(lang.to_ascii_lowercase().as_str(), "mermaid" | "dot" | "graphviz" | "chart")
}

/// Draws the diagram. Charts fit into `width` columns; graphs and sequence diagrams
/// may be up to `MAX_DIAGRAM_WIDTH` wide. `Err` explains why it cannot be drawn.
pub fn render(lang: &str, src: &str, width: usize, theme: &Theme) -> Result<Lines, String> {
    let pal = Palette::new(theme);
    match lang.to_ascii_lowercase().as_str() {
        "mermaid" => mermaid::render(src, width, &pal),
        "dot" | "graphviz" => graph::render(&dot::parse(src)?, MAX_DIAGRAM_WIDTH, &pal).map(|c| c.lines()),
        "chart" => chart::render(&chart::parse_block(src)?, width, &pal),
        other => Err(format!("unknown diagram language {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeKind;

    fn draw(lang: &str, src: &str, width: usize) -> Vec<String> {
        let theme = Theme::new(ThemeKind::Dark);
        let lines = render(lang, src, width, &theme).unwrap_or_else(|e| panic!("{e}"));
        let out: Vec<String> = lines.iter().map(|l| l.iter().map(|p| p.0.as_str()).collect()).collect();
        if std::env::var("MDR_DUMP").is_ok() {
            println!("--- {lang} ---\n{}", out.join("\n"));
        }
        out
    }

    fn fits(out: &[String], width: usize) {
        use unicode_width::UnicodeWidthStr;
        for l in out {
            assert!(l.width() <= width, "line wider than {width}: {l:?}");
        }
    }

    #[test]
    fn flowchart_top_down() {
        let out = draw("mermaid", "graph TD\nA[Start] --> B{Check}\nB -->|yes| C[Done]\nB -->|no| D[Retry]\nD --> B", 60);
        fits(&out, 60);
        let text = out.join("\n");
        for s in ["Start", "Check", "Done", "Retry", "yes", "no", "▼"] {
            assert!(text.contains(s), "missing {s}:\n{text}");
        }
        // Start is above Check.
        let row = |s: &str| out.iter().position(|l| l.contains(s)).unwrap();
        assert!(row("Start") < row("Check"));
        assert!(row("Check") < row("Done"));
    }

    #[test]
    fn flowchart_left_right() {
        let out = draw("mermaid", "graph LR\nA --> B --> C\nA --> C", 60);
        fits(&out, 60);
        assert!(out.join("").contains('▶'), "{out:#?}");
        let col = |name: &str| out.iter().find_map(|l| l.find(&format!("│ {name} │"))).unwrap();
        assert!(col("A") < col("B") && col("B") < col("C"), "{out:#?}");
    }

    #[test]
    fn flowchart_bottom_up_flips() {
        let out = draw("mermaid", "graph BT\nA --> B", 40);
        let row = |s: &str| out.iter().position(|l| l.contains(s)).unwrap();
        assert!(row("A") > row("B"));
        assert!(out.join("").contains('▲'));
    }

    #[test]
    fn wide_graph_renders_beyond_window_width() {
        let src = "graph LR\nA[a very long label here] --> B[another very long label] --> C[and a third long one]";
        let out = draw("mermaid", src, 30);
        use unicode_width::UnicodeWidthStr;
        assert!(out.iter().any(|l| l.width() > 30));
    }

    #[test]
    fn graph_over_max_width_is_an_error() {
        let theme = Theme::new(ThemeKind::Dark);
        let label = "x".repeat(60);
        let src: String = std::iter::once("graph LR".to_string())
            .chain((0..10).map(|i| format!("N{i}[{label}] --> N{}[{label}]", i + 1)))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(render("mermaid", &src, 80, &theme).is_err());
    }

    #[test]
    fn graphviz_digraph() {
        let out = draw("dot", "digraph { rankdir=TB; a -> b; a -> c; b -> d; c -> d }", 60);
        fits(&out, 60);
        assert!(out.join("\n").contains('d'));
    }

    #[test]
    fn sequence_diagram() {
        let src = "sequenceDiagram\nautonumber\nparticipant A as Alice\nA->>B: Hello Bob\nB-->>A: Hi\nloop Every minute\nA->>A: think\nend\nNote over A,B: done";
        let out = draw("mermaid", src, 70);
        fits(&out, 70);
        let text = out.join("\n");
        for s in ["Alice", "1. Hello Bob", "2. Hi", "loop", "done", "▶", "◀"] {
            assert!(text.contains(s), "missing {s}:\n{text}");
        }
    }

    #[test]
    fn charts() {
        let out = draw("chart", "title: Sales\nJan: 10\nFeb: 20\nMar: 5", 40);
        fits(&out, 40);
        assert!(out[0].contains("Sales"));
        assert!(out[2].contains("████"), "{out:#?}");
        let out = draw("chart", "type: column\nJan: 10\nFeb: 20", 40);
        fits(&out, 40);
        let out = draw("chart", "type: line\nseries: a, b\nJan: 1, 3\nFeb: 5, 2\nMar: 2, 4", 50);
        fits(&out, 50);
        assert!(out.iter().any(|l| l.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))));
        let out = draw("mermaid", "pie title Pets\n\"Dogs\" : 3\n\"Cats\" : 1", 50);
        assert!(out.join("\n").contains("75.0%"));
    }

    #[test]
    fn long_chain_does_not_overflow_stack() {
        let theme = Theme::new(ThemeKind::Dark);
        let src: String = std::iter::once("graph TD".to_string()).chain((0..250).map(|i| format!("N{i} --> N{}", i + 1))).collect::<Vec<_>>().join("\n");
        // Too tall for the canvas limit or fine: either way it must not crash.
        let _ = render("mermaid", &src, 80, &theme);
        let big: String = std::iter::once("graph TD".to_string()).chain((0..2000).map(|i| format!("N{i} --> N{}", i + 1))).collect::<Vec<_>>().join("\n");
        assert!(render("mermaid", &big, 80, &theme).is_err());
    }

    #[test]
    fn state_diagram() {
        let out = draw("mermaid", "stateDiagram-v2\n[*] --> Idle\nIdle --> Busy : start\nBusy --> Idle : done\nBusy --> [*]", 60);
        fits(&out, 60);
        assert!(out.join("\n").contains("Idle"));
    }
}
