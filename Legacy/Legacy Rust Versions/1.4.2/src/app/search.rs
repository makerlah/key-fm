use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use crate::models::Item;

pub(crate) const SEARCH_DEBOUNCE_MS: u64 = 150;
pub(crate) const SEARCH_RESULT_CAP: usize = 5000;
pub(crate) const SKIP_DIRS: &[&str] = &[
    "$Recycle.Bin",
    "System Volume Information",
    "Windows",
    "node_modules",
    ".git",
];

pub(crate) struct SearchHit {
    path: PathBuf,
    name: String,
    is_dir: bool,
    size: u64,
    mtime: Option<f64>,
}

impl SearchHit {
    pub(crate) fn into_item(self) -> Item {
        Item {
            name: self.name,
            path: self.path,
            is_dir: self.is_dir,
            size: self.size,
            mtime: self.mtime,
            hidden: false,
            extra: String::new(),
            orig: None,
            info: String::new(),
            total: 0,
        }
    }
}

pub(crate) fn run_walk(
    root: PathBuf,
    needle: String,
    show_hidden: bool,
    cancel: std::sync::Arc<AtomicBool>,
    tx: Sender<SearchHit>,
) {
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) { return; }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if cancel.load(Ordering::Relaxed) { return; }
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            let name_os = entry.file_name();
            let name = name_os.to_string_lossy();
            let is_dir = meta.is_dir();
            let hidden = name.starts_with('.')
                || crate::helpers::is_windows_hidden(Some(&meta));

            if is_dir && !hidden {
                let skip = SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s));
                if !skip { stack.push(path.clone()); }
            }
            if !show_hidden && hidden { continue; }
            if name.to_lowercase().contains(&needle) {
                let size = if is_dir { 0 } else { meta.len() };
                let mtime = meta.modified().ok().and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64())
                });
                if tx.send(SearchHit { path, name: name.into_owned(), is_dir, size, mtime }).is_err() {
                    return;
                }
            }
        }
    }
}