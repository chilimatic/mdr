//! Turns simple TeX math (`$x^2$`, `$$\frac{a}{b}$$`) into Unicode text.
//!
//! This is not a TeX engine: it knows the common commands, Greek letters, operators,
//! superscripts and subscripts, fractions, roots and a few environments. Unknown
//! commands stay as written.

/// Converts TeX math to one line of text (`display`: `\\` starts a new line).
pub fn to_unicode(tex: &str, display: bool) -> String {
    let chars: Vec<char> = tex.chars().collect();
    let mut p = P { c: &chars, i: 0, display, depth: 0 };
    let out = p.seq(None);
    // TeX ignores spaces in math; keep single spaces the author wrote.
    let mut s = String::new();
    for ch in out.chars() {
        if ch == ' ' && (s.ends_with(' ') || s.ends_with('\n') || s.is_empty()) {
            continue;
        }
        s.push(ch);
    }
    s.lines().map(str::trim).collect::<Vec<_>>().join("\n")
}

struct P<'a> {
    c: &'a [char],
    i: usize,
    display: bool,
    depth: usize,
}

const MAX_DEPTH: usize = 50;

impl P<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    /// Reads until `close` (not included) or the end.
    fn seq(&mut self, close: Option<char>) -> String {
        let mut out = String::new();
        while let Some(ch) = self.peek() {
            if Some(ch) == close {
                break;
            }
            match ch {
                '^' | '_' => {
                    self.i += 1;
                    let arg = self.arg();
                    out.push_str(&script(&arg, ch == '^'));
                }
                '&' => {
                    self.i += 1;
                    out.push(' ');
                }
                '~' => {
                    self.i += 1;
                    out.push(' ');
                }
                '\'' => {
                    self.i += 1;
                    out.push('′');
                }
                '}' => {
                    // Unbalanced: show it.
                    self.i += 1;
                    out.push('}');
                }
                _ => out.push_str(&self.atom()),
            }
        }
        out
    }

    /// One argument: a `{group}`, a command, or one character.
    fn arg(&mut self) -> String {
        while self.peek() == Some(' ') {
            self.i += 1;
        }
        self.atom()
    }

    fn atom(&mut self) -> String {
        let Some(ch) = self.peek() else { return String::new() };
        self.i += 1;
        if matches!(ch, '{' | '\\') && self.depth >= MAX_DEPTH {
            return String::new();
        }
        match ch {
            '{' => {
                self.depth += 1;
                let s = self.seq(Some('}'));
                self.depth -= 1;
                if self.peek() == Some('}') {
                    self.i += 1;
                }
                s
            }
            '\\' => {
                self.depth += 1;
                let s = self.command();
                self.depth -= 1;
                s
            }
            '\n' | '\t' => " ".into(),
            c => c.to_string(),
        }
    }

    /// An optional `[...]` argument.
    fn opt(&mut self) -> Option<String> {
        if self.peek() != Some('[') {
            return None;
        }
        self.i += 1;
        let s = self.seq(Some(']'));
        if self.peek() == Some(']') {
            self.i += 1;
        }
        Some(s)
    }

    fn name(&mut self) -> String {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        if self.i == start {
            // A one-character command such as `\,` or `\{`.
            if let Some(c) = self.peek() {
                self.i += 1;
                return c.to_string();
            }
        }
        self.c[start..self.i].iter().collect()
    }

    fn command(&mut self) -> String {
        let name = self.name();
        if let Some(s) = symbol(&name) {
            return s.to_string();
        }
        if FUNCTIONS.contains(&name.as_str()) {
            // `\sin x`: keep a space before a following letter.
            let next_letter = self.c.get(self.i..).and_then(|r| r.iter().find(|c| **c != ' ')).is_some_and(|c| c.is_alphanumeric() || *c == '\\');
            return if next_letter { format!("{name} ") } else { name };
        }
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let (a, b) = (self.arg(), self.arg());
                format!("{}/{}", paren(&a), paren(&b))
            }
            "binom" => {
                let (a, b) = (self.arg(), self.arg());
                format!("C({a}, {b})")
            }
            "sqrt" => {
                let n = self.opt();
                let x = self.arg();
                let root = match n.as_deref() {
                    None | Some("2") => "√".to_string(),
                    Some("3") => "∛".to_string(),
                    Some("4") => "∜".to_string(),
                    Some(n) => format!("{}√", script(n, true)),
                };
                format!("{root}{}", paren(&x))
            }
            "mathbb" => self.arg().chars().map(double_struck).collect(),
            "mathbf" | "mathit" | "mathrm" | "mathsf" | "mathtt" | "mathcal" | "mathfrak" | "boldsymbol" | "bm"
            | "operatorname" | "text" | "textrm" | "textit" | "textbf" | "mbox" | "displaystyle" | "textstyle" => {
                if matches!(name.as_str(), "displaystyle" | "textstyle") {
                    return String::new();
                }
                self.arg()
            }
            "hat" | "widehat" => accent(&self.arg(), '\u{302}'),
            "bar" | "overline" => accent(&self.arg(), '\u{305}'),
            "vec" => accent(&self.arg(), '\u{20d7}'),
            "dot" => accent(&self.arg(), '\u{307}'),
            "ddot" => accent(&self.arg(), '\u{308}'),
            "tilde" | "widetilde" => accent(&self.arg(), '\u{303}'),
            "underline" => accent(&self.arg(), '\u{332}'),
            "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "middle" => {
                // The delimiter follows; `\left.` means none.
                if self.peek() == Some('.') {
                    self.i += 1;
                }
                String::new()
            }
            "begin" => self.environment(),
            "end" => {
                self.arg();
                String::new()
            }
            "\\" => {
                if self.display {
                    "\n".into()
                } else {
                    "; ".into()
                }
            }
            "," | ":" | ";" | " " | ">" => " ".into(),
            "!" => String::new(),
            "{" | "}" | "%" | "$" | "#" | "&" | "_" | "|" => if name == "|" { "‖".into() } else { name },
            _ => format!("\\{name}"),
        }
    }

    /// `\begin{env} ... \end{env}`: rows joined by `;` (inline) or new lines (display).
    fn environment(&mut self) -> String {
        let env = self.arg();
        let env = env.trim_end_matches('*');
        let body_start = self.i;
        let end = format!("\\end{{{env}");
        let rest: String = self.c[body_start..].iter().collect();
        let (body, skip) = match rest.find(&end) {
            Some(k) => (rest[..k].to_string(), k + end.len()),
            None => (rest.clone(), rest.len()),
        };
        self.i = body_start + rest[..skip].chars().count();
        // Skip the closing `}` and a starred `*}`.
        while matches!(self.peek(), Some('*') | Some('}')) {
            self.i += 1;
        }
        let inner = P { c: &body.chars().collect::<Vec<_>>(), i: 0, display: self.display, depth: self.depth + 1 }.run();
        let rows: Vec<&str> = inner.split(['\n', ';']).map(str::trim).filter(|r| !r.is_empty()).collect();
        let sep = if self.display { "\n" } else { "; " };
        let joined = rows.join(sep);
        match env {
            "pmatrix" => format!("({joined})"),
            "bmatrix" => format!("[{joined}]"),
            "vmatrix" => format!("|{joined}|"),
            "cases" => format!("{{ {joined}"),
            _ => joined,
        }
    }

    fn run(mut self) -> String {
        if self.depth >= MAX_DEPTH {
            return String::new();
        }
        self.seq(None)
    }
}

/// Wraps a fraction part or root in parentheses when it has an operator or a space.
fn paren(s: &str) -> String {
    let s = s.trim();
    if s.chars().any(|c| " +-−=/·×±,<>≤≥".contains(c)) { format!("({s})") } else { s.to_string() }
}

fn accent(s: &str, mark: char) -> String {
    let mut out = String::new();
    for c in s.chars() {
        out.push(c);
        if !c.is_whitespace() {
            out.push(mark);
        }
    }
    out
}

/// Superscript or subscript characters, or `^(...)` / `_(...)` when one is missing.
fn script(s: &str, sup: bool) -> String {
    let s = s.trim();
    if sup && s == "′" {
        return "′".into();
    }
    let mapped: Option<String> = s.chars().map(|c| if sup { superscript(c) } else { subscript(c) }).collect();
    match mapped {
        Some(m) if !m.is_empty() => m,
        _ => {
            let mark = if sup { '^' } else { '_' };
            if s.chars().count() == 1 { format!("{mark}{s}") } else { format!("{mark}({s})") }
        }
    }
}

fn superscript(c: char) -> Option<char> {
    let from = "0123456789+-=()abcdefghijklmnoprstuvwxyzABDEGHIJKLMNOPRTUVWαβγδεθιφχ";
    let to = "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ᵃᵇᶜᵈᵉᶠᵍʰⁱʲᵏˡᵐⁿᵒᵖʳˢᵗᵘᵛʷˣʸᶻᴬᴮᴰᴱᴳᴴᴵᴶᴷᴸᴹᴺᴼᴾᴿᵀᵁⱽᵂᵅᵝᵞᵟᵋᶿᶥᵠᵡ";
    if c == '∗' || c == '*' {
        return Some('*');
    }
    if to.contains(c) {
        return Some(c);
    }
    from.chars().position(|f| f == c).and_then(|k| to.chars().nth(k))
}

fn subscript(c: char) -> Option<char> {
    let from = "0123456789+-=()aehijklmnoprstuvxβγρφχ";
    let to = "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ₐₑₕᵢⱼₖₗₘₙₒₚᵣₛₜᵤᵥₓᵦᵧᵨᵩᵪ";
    if to.contains(c) {
        return Some(c);
    }
    from.chars().position(|f| f == c).and_then(|k| to.chars().nth(k))
}

fn double_struck(c: char) -> char {
    match c {
        'C' => 'ℂ',
        'H' => 'ℍ',
        'N' => 'ℕ',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'Z' => 'ℤ',
        'A'..='Z' => char::from_u32(0x1D538 + (c as u32 - 'A' as u32)).unwrap_or(c),
        '0'..='9' => char::from_u32(0x1D7D8 + (c as u32 - '0' as u32)).unwrap_or(c),
        _ => c,
    }
}

const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "log", "ln", "lg",
    "exp", "lim", "liminf", "limsup", "max", "min", "sup", "inf", "det", "gcd", "arg", "deg", "dim", "ker", "Pr", "mod",
];

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ϵ",
        "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "varpi" => "ϖ",
        "rho" => "ρ",
        "varrho" => "ϱ",
        "sigma" => "σ",
        "varsigma" => "ς",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "ϕ",
        "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "ne" | "neq" => "≠",
        "ll" => "≪",
        "gg" => "≫",
        "approx" => "≈",
        "equiv" => "≡",
        "sim" => "∼",
        "simeq" => "≃",
        "cong" => "≅",
        "propto" => "∝",
        "times" => "×",
        "cdot" => "·",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "ast" => "∗",
        "star" => "⋆",
        "circ" => "∘",
        "bullet" => "•",
        "oplus" => "⊕",
        "otimes" => "⊗",
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "sum" => "∑",
        "prod" => "∏",
        "coprod" => "∐",
        "int" => "∫",
        "iint" => "∬",
        "iiint" => "∭",
        "oint" => "∮",
        "in" => "∈",
        "notin" => "∉",
        "ni" => "∋",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "supset" => "⊃",
        "supseteq" => "⊇",
        "cup" => "∪",
        "cap" => "∩",
        "bigcup" => "⋃",
        "bigcap" => "⋂",
        "setminus" => "∖",
        "emptyset" | "varnothing" => "∅",
        "forall" => "∀",
        "exists" => "∃",
        "nexists" => "∄",
        "neg" | "lnot" => "¬",
        "land" | "wedge" => "∧",
        "lor" | "vee" => "∨",
        "to" | "rightarrow" => "→",
        "leftarrow" | "gets" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" | "iff" => "⇔",
        "implies" => "⟹",
        "impliedby" => "⟸",
        "mapsto" => "↦",
        "uparrow" => "↑",
        "downarrow" => "↓",
        "ldots" | "dots" | "dotsc" => "…",
        "cdots" | "dotsb" => "⋯",
        "vdots" => "⋮",
        "ddots" => "⋱",
        "angle" => "∠",
        "perp" => "⊥",
        "parallel" => "∥",
        "mid" => "∣",
        "langle" => "⟨",
        "rangle" => "⟩",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lceil" => "⌈",
        "rceil" => "⌉",
        "lvert" | "rvert" | "vert" => "|",
        "lVert" | "rVert" | "Vert" => "‖",
        "hbar" => "ℏ",
        "ell" => "ℓ",
        "Re" => "ℜ",
        "Im" => "ℑ",
        "aleph" => "ℵ",
        "wp" => "℘",
        "prime" => "′",
        "degree" => "°",
        "therefore" => "∴",
        "because" => "∵",
        "quad" => "  ",
        "qquad" => "    ",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::to_unicode;

    fn t(s: &str) -> String {
        to_unicode(s, false)
    }

    #[test]
    fn letters_operators_and_scripts() {
        assert_eq!(t(r"E = mc^2"), "E = mc²");
        assert_eq!(t(r"\alpha + \beta \le \gamma"), "α + β ≤ γ");
        assert_eq!(t(r"x_{i+1} = x_i^{n}"), "xᵢ₊₁ = xᵢⁿ");
        assert_eq!(t(r"a^{qq}"), "a^(qq)");
        assert_eq!(t(r"e^{i\pi} + 1 = 0"), "e^(iπ) + 1 = 0");
        assert_eq!(t(r"e^{-x^2}"), "e⁻ˣ²");
        assert_eq!(t(r"\frac{\sqrt{\pi}}{2}"), "√π/2");
        assert_eq!(t(r"f'(x)"), "f′(x)");
    }

    #[test]
    fn fractions_roots_and_sets() {
        assert_eq!(t(r"\frac{1}{2}"), "1/2");
        assert_eq!(t(r"\frac{a+b}{c}"), "(a+b)/c");
        assert_eq!(t(r"\sqrt{x^2 + y^2}"), "√(x² + y²)");
        assert_eq!(t(r"\sqrt[3]{8}"), "∛8");
        assert_eq!(t(r"x \in \mathbb{R}"), "x ∈ ℝ");
        assert_eq!(t(r"\sum_{i=1}^{n} i"), "∑ᵢ₌₁ⁿ i");
        assert_eq!(t(r"\lim_{x \to 0} \frac{\sin x}{x}"), "lim_(x → 0) (sin x)/x");
    }

    #[test]
    fn environments_and_unknown_commands() {
        assert_eq!(t(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"), "(a b; c d)");
        assert_eq!(to_unicode(r"\begin{aligned} a &= 1 \\ b &= 2 \end{aligned}", true), "a = 1\nb = 2");
        assert_eq!(t(r"\foo{x}"), r"\foox");
        assert_eq!(t(r"\left( \frac{a}{b} \right)"), "( a/b )");
        assert_eq!(t(r"\hat{x}"), "x\u{302}");
    }

    #[test]
    fn hostile_input_does_not_panic() {
        let _ = t(&"{".repeat(10_000));
        let _ = t(&r"\frac".repeat(10_000));
        let _ = t(&r"\sqrt[".repeat(10_000));
        let _ = t(r"\begin{pmatrix");
        let _ = t(r"\sqrt[");
        let _ = t("^_^_\\");
    }
}
