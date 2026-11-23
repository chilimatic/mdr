# mdr

An interactive markdown viewer for the terminal. It shows a markdown file full-screen,
with styled text, syntax-highlighted code, tables, inline images and text-art diagrams.

```sh
mdr README.md
```

## Features

- Headings, wrapped paragraphs, nested and task lists, quotes, GitHub alerts
  (`> [!NOTE]`), tables with column alignment, footnotes. Front matter is hidden.
- Code blocks with syntax highlighting.
- Search, a table of contents, and jumps between headings.
- Links: select with `Tab` and open with `Enter`. A `#anchor` link jumps to the heading.
  A link to another `.md` file opens it in mdr, and `Backspace` goes back. Other links
  open in your browser or default application.
- Dark and light themes. On start, mdr picks the theme from the terminal background.
- Adjustable text width. The default is 100 columns or the terminal width, whichever is smaller.
- Watch mode: mdr reloads the file when it changes on disk and keeps your position.
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

## Usage

```text
mdr [OPTIONS] [FILE]
```

| Option | Meaning |
|---|---|
| `FILE` | Markdown file to show. Use `-` or leave it out to read from stdin. |
| `--theme auto\|dark\|light` | Color theme. Default: `auto`. |
| `--width N` | Maximum text width in columns. |
| `-w`, `--watch` | Reload when the file changes. Needs a file, not stdin. |
| `--no-images` | Do not show images inline. |
| `-p`, `--print` | Print the rendered document and exit, like `cat`. |
| `--color auto\|always\|never` | Colors in printed output. Default: `auto`. |

Examples:

```sh
mdr notes.md
mdr --watch --theme light docs/guide.md
curl -s https://example.com/README.md | mdr
```

When mdr reads from stdin, relative image paths are resolved from the current directory,
and images are drawn with block characters.

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
- `--watch` does not work in this mode.

## Keys

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
| `Enter` / `o` | Open the selected link |
| `Backspace` | Go back to the previous file |
| `T` | Switch between dark and light theme |
| `+` / `-` / `0` | Wider / narrower text / default width |
| `w` | Turn watch mode on or off |
| `r` | Reload the file |
| `Esc` | Clear the search and link selection |
| `?` | Show the key list |
| `q` / `Ctrl-c` | Quit |

The search ignores case, unless the search text has an uppercase letter.

Mouse capture is off, so you can select text with the mouse. Most terminals turn the
mouse wheel into arrow keys in full-screen programs, so the wheel scrolls too.

## Diagrams

mdr draws these fenced code blocks as text art instead of code:

| Block | Supported |
|---|---|
| ` ```mermaid ` | `graph` / `flowchart` (directions `TD`, `TB`, `BT`, `LR`, `RL`), `stateDiagram`, `sequenceDiagram`, `pie`, `xychart-beta` |
| ` ```dot ` / ` ```graphviz ` | `digraph` and `graph`, `rankdir`, labels, node shapes, `style=dashed\|dotted\|bold`, `dir`, subgraphs |
| ` ```chart ` | Bar, column and line charts (format below) |

Flowcharts support node shapes, edge labels (`-->|yes|` and `-- yes -->`), dotted
(`-.->`) and thick (`==>`) edges, chains (`A --> B --> C`) and groups (`A & B --> C`).
Sequence diagrams support `participant` / `actor` with `as`, solid and dashed arrows,
messages to self, `autonumber`, notes, and `loop` / `alt` / `opt` / `par` sections.

A graph or sequence diagram that is wider than the window is still drawn. A line above it
says so, and you scroll sideways with `h` and `l`. The status bar then shows the current
column. Search and link selection also scroll sideways to show the match or link. Charts
always fit the window, and wide tables wrap the text in their cells.

If mdr cannot draw a diagram, it shows a warning line and then the source as a normal
code block. This happens when:

- the diagram is wider than 400 columns,
- the Mermaid diagram type is not supported (for example `gantt`, `classDiagram`, `erDiagram`),
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
| `src/search.rs` | Search |
| `src/theme.rs` | Colors |
