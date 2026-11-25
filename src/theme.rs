//! Color themes for the renderer and the UI chrome.

use ratatui::style::{Color, Modifier, Style};
use two_face::theme::EmbeddedThemeName;

/// Theme selection from the command line.
#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    Auto,
    Dark,
    Light,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeKind {
    Dark,
    Light,
}

impl ThemeKind {
    pub fn toggle(self) -> Self {
        match self {
            ThemeKind::Dark => ThemeKind::Light,
            ThemeKind::Light => ThemeKind::Dark,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ThemeKind::Dark => "dark",
            ThemeKind::Light => "light",
        }
    }

    /// Picks a theme kind. A terminal background color reported by the terminal wins.
    /// Then the `COLORFGBG` environment variable is used. The default is dark.
    pub fn detect(choice: ThemeChoice, terminal_bg: Option<(u8, u8, u8)>) -> Self {
        match choice {
            ThemeChoice::Dark => return ThemeKind::Dark,
            ThemeChoice::Light => return ThemeKind::Light,
            ThemeChoice::Auto => {}
        }
        if let Some((r, g, b)) = terminal_bg {
            let luma = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
            return if luma > 128.0 { ThemeKind::Light } else { ThemeKind::Dark };
        }
        if let Ok(v) = std::env::var("COLORFGBG")
            && let Some(bg) = v.rsplit(';').next().and_then(|s| s.parse::<u8>().ok())
        {
            return if bg == 7 || bg >= 9 { ThemeKind::Light } else { ThemeKind::Dark };
        }
        ThemeKind::Dark
    }
}

pub struct Theme {
    pub kind: ThemeKind,
    pub text: Style,
    pub headings: [Style; 6],
    pub heading_rule: Style,
    pub strong: Style,
    pub emph: Style,
    pub strike: Style,
    pub code_inline: Style,
    pub math: Style,
    pub code_label: Style,
    pub link: Style,
    pub quote_bar: Style,
    pub quote_label: Style,
    pub list_marker: Style,
    pub rule: Style,
    pub table_border: Style,
    pub table_header: Style,
    pub image_alt: Style,
    pub dim: Style,
    pub search_match: Style,
    pub search_current: Style,
    pub link_selected: Style,
    pub status: Style,
    pub status_accent: Style,
    pub popup_border: Style,
    pub syntax: EmbeddedThemeName,
}

fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Theme {
    pub fn new(kind: ThemeKind) -> Self {
        let fg = |hex| Style::default().fg(rgb(hex));
        let bold = Modifier::BOLD;
        match kind {
            ThemeKind::Dark => Theme {
                kind,
                text: Style::default(),
                headings: [
                    fg(0xc678dd).add_modifier(bold),
                    fg(0x61afef).add_modifier(bold),
                    fg(0x56b6c2).add_modifier(bold),
                    fg(0x98c379).add_modifier(bold),
                    fg(0xe5c07b).add_modifier(bold),
                    fg(0xabb2bf).add_modifier(bold),
                ],
                heading_rule: fg(0x5c6370),
                strong: Style::default().add_modifier(bold),
                emph: Style::default().add_modifier(Modifier::ITALIC),
                strike: Style::default().add_modifier(Modifier::CROSSED_OUT),
                code_inline: fg(0xe5c07b).bg(rgb(0x2c313a)),
                math: fg(0x56b6c2).add_modifier(Modifier::ITALIC),
                code_label: fg(0x7f848e),
                link: fg(0x61afef).add_modifier(Modifier::UNDERLINED),
                quote_bar: fg(0x5c6370),
                quote_label: fg(0x61afef).add_modifier(bold),
                list_marker: fg(0xd19a66),
                rule: fg(0x5c6370),
                table_border: fg(0x5c6370),
                table_header: Style::default().add_modifier(bold),
                image_alt: fg(0x98c379),
                dim: fg(0x7f848e),
                search_match: Style::default().fg(Color::Black).bg(rgb(0xe5c07b)),
                search_current: Style::default().fg(Color::Black).bg(rgb(0xd19a66)).add_modifier(bold),
                link_selected: Style::default().fg(Color::Black).bg(rgb(0x61afef)),
                status: Style::default().fg(rgb(0xabb2bf)).bg(rgb(0x2c313a)),
                status_accent: Style::default().fg(Color::Black).bg(rgb(0x61afef)).add_modifier(bold),
                popup_border: fg(0x61afef),
                syntax: EmbeddedThemeName::OneHalfDark,
            },
            ThemeKind::Light => Theme {
                kind,
                text: Style::default(),
                headings: [
                    fg(0xa626a4).add_modifier(bold),
                    fg(0x4078f2).add_modifier(bold),
                    fg(0x0184bc).add_modifier(bold),
                    fg(0x50a14f).add_modifier(bold),
                    fg(0xc18401).add_modifier(bold),
                    fg(0x696c77).add_modifier(bold),
                ],
                heading_rule: fg(0xa0a1a7),
                strong: Style::default().add_modifier(bold),
                emph: Style::default().add_modifier(Modifier::ITALIC),
                strike: Style::default().add_modifier(Modifier::CROSSED_OUT),
                code_inline: fg(0xe45649).bg(rgb(0xeaeaeb)),
                math: fg(0x0184bc).add_modifier(Modifier::ITALIC),
                code_label: fg(0x696c77),
                link: fg(0x4078f2).add_modifier(Modifier::UNDERLINED),
                quote_bar: fg(0xa0a1a7),
                quote_label: fg(0x4078f2).add_modifier(bold),
                list_marker: fg(0x986801),
                rule: fg(0xa0a1a7),
                table_border: fg(0xa0a1a7),
                table_header: Style::default().add_modifier(bold),
                image_alt: fg(0x50a14f),
                dim: fg(0x696c77),
                search_match: Style::default().fg(Color::Black).bg(rgb(0xf5d76e)),
                search_current: Style::default().fg(Color::White).bg(rgb(0xc18401)).add_modifier(bold),
                link_selected: Style::default().fg(Color::White).bg(rgb(0x4078f2)),
                status: Style::default().fg(rgb(0x383a42)).bg(rgb(0xe5e5e6)),
                status_accent: Style::default().fg(Color::White).bg(rgb(0x4078f2)).add_modifier(bold),
                popup_border: fg(0x4078f2),
                syntax: EmbeddedThemeName::OneHalfLight,
            },
        }
    }
}
