# mdr demo

Markdown in the terminal: **bold**, *italic*, ~~strike~~, `inline code`, [links](https://example.com) and footnotes[^1].

> [!NOTE]
> Press `?` in mdr to see all keys, and `t` for the table of contents.

## Lists and tables

- [x] Wrapped paragraphs and nested lists
- [x] Tables with alignment
- [ ] Your next document

| Feature  | Key | Works in |
|:---------|:---:|---------:|
| Search   | `/` |  any file |
| Fold     | `z` |  sections |
| Copy     | `y` | code blocks |

## Code and math

```rust
fn main() {
    let greeting = "hello";
    println!("{greeting}, terminal");
}
```

Euler's identity is $e^{i\pi} + 1 = 0$, and the Gaussian integral is

$$
\int_{-\infty}^{\infty} e^{-x^2} dx = \sqrt{\pi}
$$

## Diagrams

```mermaid
graph LR
    A[Write] --> B{Review}
    B -->|ok| C[Ship]
    B -->|fix| A
```

```chart
type: bar
title: Lines of code
Rust: 6200
Tests: 1400
Docs: 400
```

[^1]: Footnotes link both ways: `Enter` jumps, `Backspace` comes back.
