//! Reloads the document when its file changes on disk.

use std::path::Path;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use anyhow::{Context, Result};
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};

pub struct Watcher {
    _debouncer: Debouncer<RecommendedWatcher>,
    pub rx: Receiver<()>,
}

/// Watches the parent directory, because many editors save by writing a new file and renaming it.
pub fn watch(path: &Path) -> Result<Watcher> {
    let target = path.canonicalize().context("cannot resolve file path")?;
    let dir = target.parent().context("file has no parent directory")?.to_path_buf();
    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(Duration::from_millis(150), move |res: DebounceEventResult| {
        if let Ok(events) = res
            && events.iter().any(|e| e.path == target)
        {
            let _ = tx.send(());
        }
    })?;
    debouncer.watcher().watch(&dir, RecursiveMode::NonRecursive)?;
    Ok(Watcher { _debouncer: debouncer, rx })
}
