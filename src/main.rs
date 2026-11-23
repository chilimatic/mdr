mod app;
mod diagram;
mod highlight;
mod images;
mod layout;
mod search;
mod theme;
mod ui;
mod watch;

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use ratatui_image::picker::{Capability, Picker};

use app::App;
use images::Images;
use theme::{ThemeChoice, ThemeKind};

/// Interactive terminal markdown viewer.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Markdown file to show. Use `-` or leave empty to read from stdin.
    file: Option<PathBuf>,

    /// Color theme.
    #[arg(long, value_enum, default_value_t = ThemeChoice::Auto)]
    theme: ThemeChoice,

    /// Maximum text width in columns. Default: 100 or the terminal width, whichever is smaller.
    #[arg(long)]
    width: Option<u16>,

    /// Reload automatically when the file changes.
    #[arg(short, long)]
    watch: bool,

    /// Do not show images inline.
    #[arg(long)]
    no_images: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let from_stdin = args.file.as_ref().is_none_or(|p| p.as_os_str() == "-");
    let (path, text) = if from_stdin {
        if std::io::stdin().is_terminal() {
            bail!("no input: give a markdown file, or pipe markdown into stdin (see --help)");
        }
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text).context("cannot read stdin")?;
        (None, text)
    } else {
        let path = args.file.clone().unwrap();
        let text = std::fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
        (Some(path), text)
    };
    if !std::io::stdout().is_terminal() {
        bail!("stdout is not a terminal; mdr is an interactive viewer");
    }
    if args.watch && path.is_none() {
        bail!("--watch needs a file, not stdin");
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, args, path, text, from_stdin);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, args: Args, path: Option<PathBuf>, text: String, from_stdin: bool) -> Result<()> {
    // The terminal query answers on stdin. Skip it when stdin is a pipe.
    let picker = if from_stdin { None } else { Picker::from_query_stdio().ok() };
    let terminal_bg = picker.as_ref().and_then(|p| {
        p.capabilities().iter().find_map(|c| match c {
            Capability::Background(r, g, b) => Some((*r, *g, *b)),
            _ => None,
        })
    });
    let theme = ThemeKind::detect(args.theme, terminal_bg);

    let image_picker = if args.no_images {
        None
    } else {
        Some(picker.unwrap_or_else(Picker::halfblocks))
    };

    let mut app = App::new(path, text, theme, args.width, Images::new(image_picker));
    if args.watch {
        app.toggle_watch();
    }
    app.run(terminal)
}
