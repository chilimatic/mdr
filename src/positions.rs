//! Remembers where the reader stopped in each file.
//!
//! One line per file: `<source byte offset>\t<line in block>\t<absolute path>`, most recent first.
//! Errors are ignored: a lost position is not worth an error message.

use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 500;

/// Source byte offset of a block, and the rendered line inside that block.
pub type Pos = (usize, usize);

pub struct Positions {
    file: PathBuf,
}

impl Positions {
    pub fn new(file: PathBuf) -> Self {
        Positions { file }
    }

    /// `$XDG_STATE_HOME/mdr/positions`, else `~/.local/state/mdr/positions`.
    pub fn default_location() -> Option<Self> {
        let nonempty = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let dir = nonempty("XDG_STATE_HOME").or_else(|| nonempty("HOME").map(|h| h.join(".local/state")))?;
        Some(Positions::new(dir.join("mdr").join("positions")))
    }

    fn key(path: &Path) -> Option<String> {
        let p = std::fs::canonicalize(path).ok()?;
        let s = p.to_str()?;
        (!s.contains(['\t', '\n'])).then(|| s.to_string())
    }

    fn load(&self) -> Vec<(Pos, String)> {
        let Ok(text) = std::fs::read_to_string(&self.file) else { return Vec::new() };
        text.lines()
            .filter_map(|l| {
                let mut f = l.splitn(3, '\t');
                let src = f.next()?.parse().ok()?;
                let row = f.next()?.parse().ok()?;
                Some(((src, row), f.next()?.to_string()))
            })
            .collect()
    }

    /// Position of the line that was at the top of the screen.
    pub fn get(&self, path: &Path) -> Option<Pos> {
        let key = Self::key(path)?;
        self.load().into_iter().find(|(_, p)| *p == key).map(|(n, _)| n)
    }

    /// Stores the position, or forgets it for `None` (the reader was at the top).
    pub fn set(&self, path: &Path, src: Option<Pos>) {
        let Some(key) = Self::key(path) else { return };
        let mut entries = self.load();
        let before = entries.len();
        entries.retain(|(_, p)| *p != key);
        if src.is_none() && entries.len() == before {
            return;
        }
        if let Some(n) = src {
            entries.insert(0, (n, key));
        }
        entries.truncate(MAX_ENTRIES);
        let text: String = entries.iter().map(|((n, r), p)| format!("{n}\t{r}\t{p}\n")).collect();
        let Some(dir) = self.file.parent() else { return };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        // Write a new file and rename it, so a crash never leaves half a file.
        let tmp = self.file.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, &self.file).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_updates_and_forgets() {
        let dir = std::env::temp_dir().join(format!("mdr-positions-test-{}", std::process::id()));
        let pos = Positions::new(dir.join("state/positions"));
        let a = Path::new("tests/fixtures/kitchen-sink.md");
        let b = Path::new("tests/fixtures/other.md");
        assert_eq!(pos.get(a), None);
        pos.set(a, Some((120, 0)));
        pos.set(b, Some((7, 2)));
        pos.set(a, Some((300, 1)));
        assert_eq!(pos.get(a), Some((300, 1)));
        assert_eq!(pos.get(b), Some((7, 2)));
        assert_eq!(pos.load().len(), 2);
        assert_eq!(pos.load()[0].0, (300, 1), "most recent first");
        pos.set(b, None);
        assert_eq!(pos.get(b), None);
        assert_eq!(pos.get(Path::new("does/not/exist.md")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
