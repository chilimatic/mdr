mod app;
mod diagram;
mod highlight;
mod images;
mod layout;
mod picker;
mod print;
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
use theme::{Theme, ThemeChoice, ThemeKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

/// Interactive terminal markdown viewer.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Markdown file to show, or a folder to pick a file from. Use `-` or leave empty to read from stdin.
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

    /// Print the rendered document and exit, like cat. This is automatic when stdout is not a terminal.
    #[arg(short, long)]
    print: bool,

    /// Colors in printed output. `auto` uses colors only when stdout is a terminal and NO_COLOR is not set.
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
    color: ColorChoice,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if let Some(dir) = args.file.clone().filter(|p| p.is_dir()) {
        return run_folder(args, dir);
    }

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
    let stdout_tty = std::io::stdout().is_terminal();
    if args.print || !stdout_tty {
        if args.watch {
            bail!("--watch only works in the interactive viewer, not with --print or piped output");
        }
        return print_doc(&args, path.as_deref(), &text, stdout_tty);
    }
    if args.watch && path.is_none() {
        bail!("--watch needs a file, not stdin");
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, args, path, text, from_stdin, None);
    ratatui::restore();
    result
}

fn run_folder(args: Args, dir: PathBuf) -> Result<()> {
    if args.print || !std::io::stdout().is_terminal() {
        bail!("{} is a folder; printing needs a file", dir.display());
    }
    let picker = picker::Picker::scan(&dir).with_context(|| format!("cannot read {}", dir.display()))?;
    if picker.files.is_empty() {
        bail!("no markdown files in {}", dir.display());
    }
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, args, None, String::new(), false, Some(picker));
    ratatui::restore();
    result
}

fn print_doc(args: &Args, path: Option<&std::path::Path>, text: &str, stdout_tty: bool) -> Result<()> {
    let color = match args.color {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => stdout_tty && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
    };
    // Width: --width, else the terminal width (it is known even when piped into less), else 80.
    let width = match args.width {
        Some(w) => w as usize,
        None => ratatui::crossterm::terminal::size()
            .map(|(w, _)| (w as usize).min(app::DEFAULT_MAX_WIDTH as usize))
            .unwrap_or(80),
    }
    .max(20);
    let theme = Theme::new(ThemeKind::detect(args.theme, None));
    let doc = print::render(text, path, &theme, width);
    print::write(&doc, color, &mut std::io::BufWriter::new(std::io::stdout().lock()))
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    args: Args,
    path: Option<PathBuf>,
    text: String,
    from_stdin: bool,
    folder: Option<picker::Picker>,
) -> Result<()> {
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
    if let Some(pk) = folder {
        app.start_picker(pk, args.watch);
    } else if args.watch {
        app.toggle_watch();
    }
    app.run(terminal)
}
