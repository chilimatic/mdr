# mdr

An interactive markdown viewer for the terminal. It shows a markdown file full-screen,
with styled text, syntax-highlighted code, tables, inline images and text-art diagrams.

```sh
mdr README.md
```

## Features

- Headings, wrapped paragraphs, nested and task lists, quotes, GitHub alerts
  (`> [!NOTE]`), tables with column alignment, footnotes. Front matter is hidden.
- Code blocks with syntax highlighting. `y` copies a code block to the clipboard.
- Math: `$E = mc^2$` and `$$...$$` blocks are shown as Unicode text (E = mc²).
- Fold sections with `z` to see the outline of a long document.
- Search, a table of contents, and jumps between headings.
- Links: select with `Tab` and open with `Enter`. A `#anchor` link jumps to the heading.
  A link to another `.md` file opens it in mdr, and `Backspace` goes back. Other links
  open in your browser or default application.
- Dark and light themes. On start, mdr picks the theme from the terminal background.
- Adjustable text width. The default is 100 columns or the terminal width, whichever is smaller.
- Watch mode: mdr reloads the file when it changes on disk and keeps your position.
- `e` opens the file in your editor at the line you are reading. mdr reloads it when the
  editor closes.
- mdr remembers where you stopped in each file and opens it there next time.
- Folders: `mdr docs/` lists the markdown files, filters them by name and searches their text.
- Inline images through the kitty, sixel or iTerm2 graphics protocols. In other terminals
  mdr draws images with block characters.
- Diagrams and charts drawn as text: Mermaid, Graphviz DOT, and a simple chart format.
  See [Diagrams](#diagrams).

## Install

You need Rust 1.90 or newer.

```sh
cargo install --locked --path .
```

This puts `mdr` in `~/.cargo/bin`. If that folder is not on your `PATH`, install into a
folder that is, for example:

```sh
cargo install --locked --path . --root ~/.local   # installs ~/.local/bin/mdr
```

Run the same command again to update. To remove: `cargo uninstall mdr` (add the same `--root`).

### Shell completions and man page

mdr prints them itself. For example:

```sh
mdr --completions bash > ~/.local/share/bash-completion/completions/mdr
mdr --completions zsh > ~/.zfunc/_mdr    # ~/.zfunc must be in $fpath
mdr --completions fish > ~/.config/fish/completions/mdr.fish
mkdir -p ~/.local/share/man/man1 && mdr --man > ~/.local/share/man/man1/mdr.1
```

`--completions` also knows `elvish` and `powershell`.

## Usage

```text
mdr [OPTIONS] [FILE]
```

| Option | Meaning |
|---|---|
| `FILE` | Markdown file to show, or a folder to pick a file from. Use `-` or leave it out to read from stdin. |
| `--theme auto\|dark\|light` | Color theme. Default: `auto`. |
| `--width N` | Maximum text width in columns. |
| `-w`, `--watch` | Reload when the file changes. Needs a file, not stdin. |
| `--no-images` | Do not show images inline. |
| `-p`, `--print` | Print the rendered document and exit, like `cat`. |
| `--color auto\|always\|never` | Colors in printed output. Default: `auto`. |
| `--no-config` | Ignore the config file. |
| `--completions SHELL` | Print a shell completion script and exit. |
| `--man` | Print the man page and exit. |

Examples:

```sh
mdr notes.md
mdr --watch --theme light docs/guide.md
curl -s https://example.com/README.md | mdr
```

When mdr reads from stdin, relative image paths are resolved from the current directory,
and images are drawn with block characters.

### Config file

mdr reads `~/.config/mdr/config.toml` (or `$XDG_CONFIG_HOME/mdr/config.toml`, or the file in
`$MDR_CONFIG`). All settings are optional. Options on the command line win over the file.

```toml
theme = "auto"      # auto, dark or light
width = 100         # maximum text width in columns
images = true       # show images inline
color = "auto"      # colors in printed output: auto, always or never
editor = "nvim"     # editor for `e`; default: $VISUAL, then $EDITOR, then vi
resume = true       # remember the reading position of each file
key_bar = true      # show the row with the keys that work now
```

An unknown setting or a wrong value is an error, and mdr names the line.

### Folders

`mdr docs/` shows a list of the markdown files (`.md`, `.markdown`) in the folder and its
subfolders. Hidden folders, `target/` and `node_modules/` are skipped. The search goes at
most 8 levels deep and lists at most 5000 files.

| Key | Action |
|---|---|
| `j` `k` / `↓` `↑` | Move the selection |
| `g` `G` / `Space` `b` | First / last / page down / page up |
| `/` | Filter by name. Every word you type must appear in the path; case is ignored. `Enter` keeps the filter, `Esc` clears it. |
| `s` | Search the text of all files. Type the text and press `Enter`. |
| `p` | Show or hide the preview |
| `Enter` / `o` / `l` / `→` | Open the file |
| `q` | Quit |

The text search shows one line for each line that contains the text: the file, the line
number and the line. `Enter` opens the file at that place, with the text selected as a search,
so `n` and `N` go to the next matches in that file. `s` starts a new search, and `Esc` goes back
to the file list. Like the search in a document, the search ignores case unless the text has an
uppercase letter. It stops after 1000 lines, and it skips files larger than 4 MB.

When the window is at least 90 columns wide, a preview of the selected file is shown on the
right. In the search results, the preview shows the place of the match, with the text marked.

In an open file, `Backspace` goes back through the files you reached with links, and then
to the list or the search results. With `--watch`, each file you open from the list is watched.

### Printed output

When stdout is not a terminal, or with `--print`, mdr does not open the viewer. It prints
the rendered document and exits:

```sh
mdr notes.md > notes.txt               # plain text
mdr notes.md | grep -n TODO            # plain text
mdr --color always notes.md | less -R  # keep the colors
mdr --print notes.md                   # print to the terminal, with colors
```

- Colors: with `--color auto`, mdr uses colors only when it prints to a terminal and the
  `NO_COLOR` environment variable is not set.
- Width: `--width` if you give it, otherwise the terminal width (at most 100), otherwise 80.
- Images print as their alt text. Diagrams print as text art, at full width.
- `--watch` does not work in this mode, and you cannot print a folder.

## Keys

The bottom row shows the keys that work at the moment, for example `n`/`N` while a search is
active or `⏎ open link` while a link is selected. `?` shows all keys.

| Key | Action |
|---|---|
| `j` `k` / `↓` `↑` | Scroll one line |
| `d` `u` / `Ctrl-d` `Ctrl-u` | Scroll half a page |
| `Space` `f` `PgDn` / `b` `PgUp` | Scroll one page (`Ctrl-f` / `Ctrl-b` also work) |
| `g` `Home` / `G` `End` | Go to the top / bottom |
| `h` `l` / `←` `→` | Scroll sideways (only when the document is wider than the window) |
| `]` / `[` | Next / previous heading |
| `t` | Table of contents (`j`/`k` to move, `Enter` to jump, `Esc` to close) |
| `/` | Search. Type the text and press `Enter`. |
| `n` / `N` | Next / previous match |
| `Tab` / `Shift-Tab` | Select the next / previous link |
| `Enter` / `o` | Open the selected link. A footnote reference `[^1]` jumps to the footnote, and the label in front of the footnote jumps back. |
| `Backspace` | Go back: to the place before a jump in the file (link, footnote, contents), then to the previous file, then to the file list |
| `y` | Copy the first code block on the screen. Press `y` again within 3 seconds to copy the next one. |
| `z` / `Z` | Fold or unfold the section at the top of the screen / fold or unfold all sections |
| `T` | Switch between dark and light theme |
| `+` / `-` / `0` | Wider / narrower text / default width |
| `w` | Turn watch mode on or off |
| `r` | Reload the file |
| `e` | Edit the file in `$VISUAL` or `$EDITOR` (default `vi`), at the line at the top of the screen |
| `Esc` | Clear the search and link selection |
| `?` | Show the key list |
| `q` / `Ctrl-c` | Quit |

The search ignores case, unless the search text has an uppercase letter.

### Copying code

`y` copies the code of a code block (for a diagram, its source). With a graphical session,
mdr uses `wl-copy`, `xclip` or `xsel`, whichever is installed. Otherwise, for example over ssh,
mdr asks the terminal to copy with the OSC 52 escape sequence. Most terminals support it; in tmux,
set `set -g set-clipboard on`.

### Folding

`z` folds the section whose heading is at the top of the screen: mdr shows only the heading and
the number of hidden lines. `z` on that heading opens it again. `Z` folds all sections at the
main level and below, so you see the outline; `Z` again opens all. Search and links skip folded
text. Folds stay when the window or the text width changes, and are reset when you open another
file.

### Editor

`e` starts the editor set in the config file, else `$VISUAL`, else `$EDITOR`, else `vi`. The shell runs the command, so
it can contain options and quotes. For vi, Vim, Neovim, nano, Emacs, micro, Kakoune, Helix,
Sublime Text, Zed and VS Code, mdr opens the file at the line at the top of the screen. In code
blocks this is the exact line; in tables and wrapped paragraphs it is a line close to it. Other
editors open the file at the start. A GUI editor must wait
until you close the file, for example `EDITOR="code --wait"`. When the editor closes, mdr reloads
the file.

### Reading position

When you leave a file or quit, mdr stores the position in
`$XDG_STATE_HOME/mdr/positions` (default `~/.local/state/mdr/positions`). The next time you
open the file from the command line or the file list, mdr starts there and shows "back where you
stopped". A file you open with a link starts at the top or at the link's heading. Press `g` to go to the top.
If you leave a file at the top, mdr forgets its position. mdr keeps the last 500 files. To
forget all positions, delete the file.

Mouse capture is off, so you can select text with the mouse. Most terminals turn the
mouse wheel into arrow keys in full-screen programs, so the wheel scrolls too.

## Math

mdr shows TeX math as Unicode text: inline math `$...$` in the line, and `$$...$$` blocks
centered on their own lines. It knows Greek letters, the common operators and arrows,
superscripts and subscripts (`x^2` → x², `a_{i+1}` → aᵢ₊₁), `\frac`, `\sqrt`, `\mathbb`,
accents such as `\hat`, text commands, functions such as `\sin`, and the `matrix`, `pmatrix`,
`bmatrix`, `cases` and `aligned` environments. Where Unicode has no superscript or subscript
character, mdr writes `^(...)` or `_(...)`. Fractions are written as `a/b`. Commands that mdr
does not know are shown as written.

## Diagrams

mdr draws these fenced code blocks as text art instead of code:

| Block | Supported |
|---|---|
| ` ```mermaid ` | `graph` / `flowchart` (directions `TD`, `TB`, `BT`, `LR`, `RL`), `stateDiagram`, `sequenceDiagram`, `classDiagram`, `erDiagram`, `gantt`, `pie`, `xychart-beta` |
| ` ```dot ` / ` ```graphviz ` | `digraph` and `graph`, `rankdir`, labels, node shapes, `style=dashed\|dotted\|bold`, `dir`, subgraphs |
| ` ```chart ` | Bar, column and line charts (format below) |

Flowcharts support node shapes, edge labels (`-->|yes|` and `-- yes -->`), dotted
(`-.->`) and thick (`==>`) edges, chains (`A --> B --> C`) and groups (`A & B --> C`).
Sequence diagrams support `participant` / `actor` with `as`, solid and dashed arrows,
messages to self, `autonumber`, notes, and `loop` / `alt` / `opt` / `par` sections.
Class diagrams draw each class as a box with its members. They support generics
(`List~T~`), `<<annotations>>`, cardinalities and the usual relations. A relation that is
not a plain arrow gets its name on the line (`inherits`, `◆ composes`, `◇ aggregates`).
ER diagrams draw entities with their attributes (`name: type PK`) and label each
relationship with its cardinality, for example `places (1 to 0..n)`. Dotted lines are
non-identifying relationships. Attribute comments are not shown.
Gantt charts draw one bar per task on a day scale that fits the window. They support
`title`, `section`, the tags `done`, `active`, `crit` and `milestone`, dates, `after id`
and durations (`3d`, `2w`, `12h`). Only `dateFormat YYYY-MM-DD` works. `axisFormat`,
`excludes` and `todayMarker` are ignored.

A graph, class, ER or sequence diagram that is wider than the window is still drawn. A line above it
says so, and you scroll sideways with `h` and `l`. The status bar then shows the current
column. Search and link selection also scroll sideways to show the match or link. Charts
always fit the window, and wide tables wrap the text in their cells.

If mdr cannot draw a diagram, it shows a warning line and then the source as a normal
code block. This happens when:

- the diagram is wider than 400 columns,
- the Mermaid diagram type is not supported (for example `journey`, `mindmap`, `gitGraph`),
- the source has a syntax error,
- the diagram is very large (more than 300 nodes, 1000 edges or 1000 messages).

Mermaid `subgraph` boxes and styling lines (`classDef`, `class`, `style`, `linkStyle`)
are ignored. The nodes and edges inside a subgraph are still drawn.

### Chart format

````text
```chart
type: line            # bar (default), column or line
title: Latency (ms)   # optional
series: p50, p95      # optional series names
08:00: 20, 80
10:00: 25, 120
12:00: 40, 200
```
````

Each data line is a label, a colon, and one or more numbers separated by commas or
spaces. A label can contain colons (`08:00`). You can also separate the label with a
comma or `|` instead of a colon. Each number column is one series. A chart can have up
to 500 rows and 8 series.

- `bar`: one horizontal bar per label, with the value at the end.
- `column`: vertical columns, grouped by label.
- `line`: a line chart drawn with braille dots. It needs at least 2 points.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

To see rendered output in the tests, set `MDR_DUMP=1`:

```sh
MDR_DUMP=1 cargo test diagram -- --nocapture --test-threads=1
```

`tests/fixtures/kitchen-sink.md` and `tests/fixtures/diagrams.md` contain every supported
element. Open them with `cargo run -- tests/fixtures/diagrams.md` to check changes by eye.

| Path | Contents |
|---|---|
| `src/main.rs` | Command line and terminal setup |
| `src/app.rs` | Application state and key handling |
| `src/layout.rs` | Markdown to styled lines (wrapping, lists, tables, code) |
| `src/ui.rs` | Drawing: text, images, status bar, popups |
| `src/diagram/` | Mermaid, DOT and chart parsers, graph layout, text canvas |
| `src/highlight.rs` | Syntax highlighting |
| `src/images.rs` | Image protocol detection and image cache |
| `src/watch.rs` | File watching |
| `src/picker.rs` | File list for folders |
| `src/grep.rs` | Text search across the files of a folder |
| `src/positions.rs` | Stored reading positions |
| `src/config.rs` | Config file |
| `src/clipboard.rs` | Copying to the clipboard |
| `src/math.rs` | TeX math to Unicode |
| `src/search.rs` | Search |
| `src/theme.rs` | Colors |
