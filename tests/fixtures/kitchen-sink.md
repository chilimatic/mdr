---
title: front matter is hidden
---

# Kitchen Sink

A paragraph with **bold**, *italic*, ~~strike~~, `inline code`, and a [link to Rust](https://www.rust-lang.org). It is long enough to wrap across several lines at normal widths, so wrapping can be checked.

Jump to [the table section](#tables) or open [the other file](other.md).

## Lists

- First item
- Second item with a longer text that wraps onto the next line when the terminal is narrow
  - Nested item
    - Deeper item
- [ ] open task
- [x] done task

1. one
2. two
3. three

## Quotes

> A plain quote with *emphasis*.
>
> > A nested quote.

> [!NOTE]
> GitHub-style alert.

## Code

```rust
fn main() {
    let greeting = "hello";
    println!("{greeting}, world");
}
```

```
plain code block without a language
```

## Tables

| Name    | Qty | Note                                   |
|:--------|----:|:--------------------------------------:|
| apples  |   3 | fresh                                  |
| bananas |  12 | a much longer note that has to wrap in a narrow terminal |

## Image

![gradient](gradient.png)

Remote image: ![badge](https://example.com/badge.svg)

---

Footnote ref[^1].

[^1]: The footnote text.

### Deep heading

#### Level four

Done. Line one  
line two after a hard break.
