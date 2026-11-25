mod app;
mod clipboard;
mod config;
mod diagram;
mod grep;
mod highlight;
mod images;
mod layout;
mod math;
mod picker;
mod positions;
mod print;
mod search;
mod theme;
mod ui;
mod watch;

use std::io::{IsTerminal, Read};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser};
use ratatui_image::picker::{Capability, Picker};

use app::App;
use config::{ColorChoice, Config};
use images::Images;
use theme::{Theme, ThemeChoice, ThemeKind};

/// Interactive terminal markdown viewer.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Markdown file to show, or a folder to pick a file from. Use `-` or leave empty to read from stdin.
    file: Option<PathBuf>,

    /// Color theme. Default: `auto`.
    #[arg(long, value_enum)]
    theme: Option<ThemeChoice>,

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

    /// Colors in printed output. `auto` (default) uses colors only when stdout is a terminal and NO_COLOR is not set.
    #[arg(long, value_enum)]
    color: Option<ColorChoice>,

    /// Print a shell completion script and exit.
    #[arg(long, value_enum, value_name = "SHELL", exclusive = true)]
    completions: Option<clap_complete::Shell>,

    /// Print the man page and exit.
    #[arg(long, exclusive = true)]
    man: bool,

    /// Ignore the config file.
    #[arg(long)]
    no_config: bool,
}

/// Command line options merged with the config file.
struct Settings {
    theme: ThemeChoice,
    width: Option<u16>,
    images: bool,
    color: ColorChoice,
    watch: bool,
    print: bool,
    editor: Option<String>,
    resume: bool,
    key_bar: bool,
}

impl Settings {
    fn new(args: &Args, cfg: Config) -> Self {
        Settings {
            theme: args.theme.or(cfg.theme).unwrap_or(ThemeChoice::Auto),
            width: args.width.or(cfg.width),
            images: !args.no_images && cfg.images.unwrap_or(true),
            color: args.color.or(cfg.color).unwrap_or(ColorChoice::Auto),
            watch: args.watch,
            print: args.print,
            editor: cfg.editor.filter(|e| !e.trim().is_empty()),
            resume: cfg.resume.unwrap_or(true),
            key_bar: cfg.key_bar.unwrap_or(true),
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(shell) = args.completions {
        clap_complete::generate(shell, &mut Args::command(), "mdr", &mut std::io::stdout());
        return Ok(());
    }
    if args.man {
        clap_mangen::Man::new(Args::command()).render(&mut std::io::stdout())?;
        return Ok(());
    }
    let cfg = if args.no_config { Config::default() } else { Config::load()? };
    let set = Settings::new(&args, cfg);

    if let Some(dir) = args.file.clone().filter(|p| p.is_dir()) {
        return run_folder(set, dir);
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
    if set.print || !stdout_tty {
        if set.watch {
            bail!("--watch only works in the interactive viewer, not with --print or piped output");
        }
        return print_doc(&set, path.as_deref(), &text, stdout_tty);
    }
    if set.watch && path.is_none() {
        bail!("--watch needs a file, not stdin");
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, set, path, text, from_stdin, None);
    ratatui::restore();
    result
}

fn run_folder(set: Settings, dir: PathBuf) -> Result<()> {
    if set.print || !std::io::stdout().is_terminal() {
        bail!("{} is a folder; printing needs a file", dir.display());
    }
    let picker = picker::Picker::scan(&dir).with_context(|| format!("cannot read {}", dir.display()))?;
    if picker.files.is_empty() {
        bail!("no markdown files in {}", dir.display());
    }
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, set, None, String::new(), false, Some(picker));
    ratatui::restore();
    result
}

fn print_doc(set: &Settings, path: Option<&std::path::Path>, text: &str, stdout_tty: bool) -> Result<()> {
    let color = match set.color {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => stdout_tty && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
    };
    // Width: --width, else the terminal width (it is known even when piped into less), else 80.
    let width = match set.width {
        Some(w) => w as usize,
        None => ratatui::crossterm::terminal::size()
            .map(|(w, _)| (w as usize).min(app::DEFAULT_MAX_WIDTH as usize))
            .unwrap_or(80),
    }
    .max(20);
    let theme = Theme::new(ThemeKind::detect(set.theme, None));
    let doc = print::render(text, path, &theme, width);
    print::write(&doc, color, &mut std::io::BufWriter::new(std::io::stdout().lock()))
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    set: Settings,
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
    let theme = ThemeKind::detect(set.theme, terminal_bg);

    let image_picker = if set.images { Some(picker.unwrap_or_else(Picker::halfblocks)) } else { None };

    let mut app = App::new(path, text, theme, set.width, Images::new(image_picker));
    if set.resume {
        app.positions = positions::Positions::default_location();
    }
    app.editor = set.editor;
    app.key_bar = set.key_bar;
    if let Some(pk) = folder {
        app.start_picker(pk, set.watch);
    } else if set.watch {
        app.toggle_watch();
    }
    app.run(terminal)
}
