//! File list for `mdr DIR`: finds markdown files under a folder and filters them.

use std::path::{Path, PathBuf};

use crate::grep::Grep;

const MAX_DEPTH: usize = 8;
const MAX_FILES: usize = 5000;
const MAX_DIRS: usize = 20_000;
const SKIP_DIRS: [&str; 2] = ["target", "node_modules"];

pub struct Picker {
    pub root: PathBuf,
    /// Paths relative to `root`, sorted.
    pub files: Vec<PathBuf>,
    pub filter: String,
    pub filtering: bool,
    /// Index into `visible()`.
    pub selected: usize,
    /// Text search across the files, shown instead of the file list.
    pub grep: Option<Grep>,
}

fn is_markdown(p: &Path) -> bool {
    p.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

impl Picker {
    /// Walks `root` (hidden folders, `target/` and `node_modules/` are skipped).
    pub fn scan(root: &Path) -> std::io::Result<Self> {
        let mut files = Vec::new();
        let mut stack = vec![(root.to_path_buf(), 0usize)];
        let mut dirs = 0;
        while let Some((dir, depth)) = stack.pop() {
            // Stop at the file limit, and do not walk a whole home folder looking for files.
            dirs += 1;
            if files.len() >= MAX_FILES || dirs > MAX_DIRS {
                break;
            }
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                // The root must be readable; unreadable subfolders are skipped.
                Err(e) if depth == 0 => return Err(e),
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') {
                    continue;
                }
                let Ok(ft) = entry.file_type() else { continue };
                if ft.is_dir() {
                    if depth < MAX_DEPTH && !SKIP_DIRS.contains(&name.as_ref()) {
                        stack.push((path, depth + 1));
                    }
                } else if is_markdown(&path)
                    && files.len() < MAX_FILES
                    && let Ok(rel) = path.strip_prefix(root)
                {
                    files.push(rel.to_path_buf());
                }
            }
        }
        files.sort();
        Ok(Picker { root: root.to_path_buf(), files, filter: String::new(), filtering: false, selected: 0, grep: None })
    }

    /// Indices into `files` that match the filter. Every word must appear, case is ignored.
    pub fn visible(&self) -> Vec<usize> {
        let words: Vec<String> = self.filter.split_whitespace().map(str::to_lowercase).collect();
        (0..self.files.len())
            .filter(|&i| {
                let p = self.files[i].to_string_lossy().to_lowercase();
                words.iter().all(|w| p.contains(w.as_str()))
            })
            .collect()
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        let v = self.visible();
        v.get(self.selected).map(|&i| self.root.join(&self.files[i]))
    }

    pub fn clamp(&mut self) {
        let n = self.visible().len();
        self.selected = self.selected.min(n.saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_fixture_files_and_filters() {
        let mut p = Picker::scan(Path::new("tests/fixtures")).unwrap();
        let names: Vec<String> = p.files.iter().map(|f| f.to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["diagrams.md", "kitchen-sink.md", "other.md"]);
        p.filter = "SINK kit".into();
        assert_eq!(p.visible(), vec![1]);
        p.filter = "nothing".into();
        assert!(p.visible().is_empty());
        p.clamp();
        assert_eq!(p.selected_path(), None);
    }

    #[test]
    fn skips_hidden_and_build_folders() {
        let root = std::env::temp_dir().join(format!("mdr-picker-test-{}", std::process::id()));
        for d in ["a", ".git", "target", "a/b"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["top.md", "a/one.markdown", "a/b/two.md", ".git/x.md", "target/y.md", "a/notes.txt"] {
            std::fs::write(root.join(f), "x").unwrap();
        }
        let p = Picker::scan(&root).unwrap();
        let names: Vec<String> = p.files.iter().map(|f| f.to_string_lossy().into_owned()).collect();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(names, vec!["a/b/two.md", "a/one.markdown", "top.md"]);
    }
}
