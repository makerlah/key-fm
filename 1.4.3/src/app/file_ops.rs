use crate::app::KeyFmApp;
use crate::app::types::ClipItem;
use crate::constants::{THIS_PC, RECYCLE, QUICK_ACCESS, VIRTUAL};
use crate::helpers::{same_loc, unique_path, move_path, copy_any, delete_permanently, is_windows_hidden};
use crate::drives;
use crate::trash;
use std::path::PathBuf;
use crate::models::Item;

impl KeyFmApp {
    pub(crate) fn load_this_pc(&mut self) {
        for (label, mount, free, total) in drives::scan_drives_info() {
            self.entries.push(Item {
                name: label, path: mount, is_dir: true, size: free, mtime: None,
                hidden: false, extra: String::new(), orig: None, info: String::new(), total,
            });
        }
    }

    pub(crate) fn load_recycle(&mut self) {
        if !cfg!(windows) {
            self.status_msg = "The Recycle Bin view is only available on Windows".to_string();
            return;
        }
        for e in trash::list_recycle_bin() {
            self.entries.push(Item {
                name: e.name, path: e.path, is_dir: e.is_dir, size: e.size,
                mtime: e.deleted, hidden: false, extra: String::new(),
                orig: Some(PathBuf::from(e.original)), info: String::new(), total: 0,
            });
        }
    }

    pub(crate) fn load_quick_access(&mut self) {
        let pins = self.config.pins.clone();
        let known = self.known_cache.clone();
        let mut seen: Vec<String> = Vec::new();

        for pin in &pins {
            if VIRTUAL.contains(&pin.as_str()) { continue; }
            let path = PathBuf::from(pin);
            let Ok(meta) = path.symlink_metadata() else { continue };
            let is_dir = meta.is_dir();
            let name = path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| pin.clone());
            let size = if is_dir { 0 } else { meta.len() };
            let mtime = meta.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64()));
            seen.push(crate::helpers::norm(pin));
            self.entries.push(Item {
                name, path, is_dir, size, mtime, hidden: false,
                extra: String::new(), orig: None, info: String::new(), total: 0,
            });
        }

        for (label, path) in &known {
            let path_str = path.to_string_lossy().to_string();
            if VIRTUAL.contains(&path_str.as_str()) { continue; }
            let norm = crate::helpers::norm(&path_str);
            if seen.iter().any(|s| s == &norm) { continue; }
            let Ok(meta) = path.metadata() else { continue };
            if !meta.is_dir() { continue; }
            let mtime = meta.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64()));
            self.entries.push(Item {
                name: label.to_string(), path: path.clone(), is_dir: true, size: 0, mtime, hidden: false,
                extra: String::new(), orig: None, info: String::new(), total: 0,
            });
        }
    }

    pub(crate) fn copy_or_cut(&mut self, mode: &str) {
        if self.cwd == THIS_PC || self.cwd == QUICK_ACCESS || (self.cwd == RECYCLE && mode == "cut") {
            self.status_msg = "Not available in this view".to_string();
            return;
        }
        let t = self.targets();
        if !t.is_empty() {
            self.clipboard.clear();
            let src: &[Item] = if self.search_active() { &self.search_results } else { &self.entries };
            for idx in t {
                if let Some(item) = src.get(idx) {
                    let name = item.orig.as_ref()
                        .and_then(|o| o.file_name())
                        .or_else(|| item.path.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| item.name.clone());
                    self.clipboard.push(ClipItem { path: item.path.clone(), name });
                }
            }
            self.clip_mode = mode.to_string();
        }
    }

    pub(crate) fn paste(&mut self) {
        if self.is_virtual_view() {
            self.status_msg = "Cannot paste into this view".to_string();
            return;
        }
        if self.clipboard.is_empty() { return; }
        let dest_dir = PathBuf::from(&self.cwd);
        let cut = self.clip_mode == "cut";
        let items = self.clipboard.clone();

        let mut done = 0usize;
        let mut failed: Vec<ClipItem> = Vec::new();
        let mut missing = 0usize;
        let mut first_err: Option<String> = None;

        for item in &items {
            let src = &item.path;
            let Ok(meta) = src.symlink_metadata() else {
                missing += 1;
                first_err.get_or_insert(format!("\"{}\" no longer exists", item.name));
                continue;
            };
            let same_folder = src.parent().map_or(false, |p| same_loc(&p.to_string_lossy(), &self.cwd));
            if cut && same_folder { continue; }
            let dest = unique_path(&dest_dir, &item.name, meta.is_dir());
            let res = if cut { move_path(src, &dest) } else { copy_any(src, &dest) };
            match res {
                Ok(()) => done += 1,
                Err(e) => {
                    first_err.get_or_insert(format!("{}: {}", item.name, e));
                    failed.push(item.clone());
                }
            }
        }
        if cut { self.clipboard = failed.clone(); }
        self.refresh_dir();
        let nfail = failed.len() + missing;
        self.status_msg = match first_err {
            Some(err) if nfail > 0 => format!("Pasted {}, {} failed ({})", done, nfail, err),
            _ => String::new(),
        };
    }

    pub(crate) fn delete_selected(&mut self, permanent: bool) {
        let is_bin = self.cwd == RECYCLE;
        if self.is_virtual_view() && !is_bin {
            self.status_msg = "Cannot delete from this view".to_string();
            return;
        }

        let src: &[Item] = if self.search_active() { &self.search_results } else { &self.entries };
        // Snapshot paths once — targets() uses indices into rendered_entries(),
        // which must not be mutated until we're finished.
        let paths: Vec<PathBuf> = self.targets().into_iter()
            .filter_map(|i| src.get(i).map(|e| e.path.clone()))
            .collect();
        if paths.is_empty() { return; }

        let mut failed: Vec<PathBuf> = Vec::new();
        let mut first_err: Option<String> = None;
        let mut ok_count = 0usize;

        for (i, path) in paths.iter().enumerate() {
            let force_permanent = permanent || is_bin;

            let res = if force_permanent {
                let del_res = delete_permanently(path);

                // Best-effort $I companion cleanup for Recycle Bin items.
                if is_bin {
                    if let Some(fname) = path.file_name() {
                        let fname_str = fname.to_string_lossy();
                        if let Some(rest) = fname_str.strip_prefix("$R") {
                            let i_path = path.with_file_name(format!("$I{}", rest));
                            let r_gone = del_res.is_ok()
                                || del_res.as_ref().err()
                                    .map_or(false, |e| e.kind() == std::io::ErrorKind::NotFound);
                            if r_gone {
                                let _ = delete_permanently(&i_path);
                            }
                        }
                    }
                }
                del_res
            } else {
                trash::move_to_trash(path)
            };

            match res {
                Ok(()) => ok_count += 1,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    failed.extend(paths[i..].iter().cloned());
                    first_err.get_or_insert("cancelled".to_string());
                    break;
                }
                Err(e) => {
                    let what = if force_permanent { "Delete" } else { "Trash" };
                    first_err.get_or_insert(format!("{} failed for \"{}\": {}", what, path.display(), e));
                    failed.push(path.clone());
                }
            }
        }

        self.refresh_dir();

        if failed.is_empty() {
            // Explicit success feedback so a stale error doesn't linger.
            self.status_msg = if ok_count == 1 {
                String::new()
            } else {
                format!("Deleted {} items", ok_count)
            };
        } else {
            let sel: Vec<usize> = self.rendered_entries().iter().enumerate()
                .filter(|(_, e)| failed.contains(&e.path))
                .map(|(i, _)| i)
                .collect();
            self.selected_item = sel.first().copied();
            self.selected_items = sel;
            self.status_msg = format!(
                "{} of {} item(s) not deleted: {}",
                failed.len(), paths.len(), first_err.unwrap_or_default()
            );
        }
    }

    pub(crate) fn reload(&mut self, keep_selection: bool) {
        self.cancel_search();
        let kept_item: Option<PathBuf> = if keep_selection {
            self.selected_item.and_then(|i| self.rendered_entries().get(i)).map(|e| e.path.clone())
        } else { None };
        let kept_multi: Vec<PathBuf> = if keep_selection {
            self.selected_items.iter()
                .filter_map(|&i| self.rendered_entries().get(i))
                .map(|e| e.path.clone()).collect()
        } else { Vec::new() };

        self.entries.clear();
        if self.cwd == THIS_PC { self.load_this_pc(); }
        else if self.cwd == RECYCLE { self.load_recycle(); }
        else if self.cwd == QUICK_ACCESS { self.load_quick_access(); }

        let is_virtual = self.is_virtual_view();
        let path = std::path::Path::new(&self.cwd);
        let read_dir_res = if is_virtual { None } else { Some(std::fs::read_dir(path)) };
        if let Some(Err(e)) = &read_dir_res {
            self.status_msg = format!("Cannot read {}: {}", self.cwd, e);
        }
        if let Some(Ok(read_dir)) = read_dir_res {
            for entry in read_dir.flatten() {
                let path = entry.path();
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let metadata = entry.metadata().ok();
                let hidden = name.starts_with('.') || is_windows_hidden(metadata.as_ref());
                if !self.config.show_hidden && hidden { continue; }
                let is_link = metadata.as_ref().map_or(false, |m| m.file_type().is_symlink());
                let metadata = if is_link { std::fs::metadata(&path).ok().or(metadata) } else { metadata };
                let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = metadata.as_ref().map(|m| if is_dir { 0 } else { m.len() }).unwrap_or(0);
                let mtime = metadata.as_ref().and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64()));
                self.entries.push(Item {
                    name, path, is_dir, size, mtime, hidden,
                    extra: String::new(), orig: None, info: String::new(), total: 0,
                });
            }
        }
        self.entries.sort_by(|a, b| {
            if a.is_dir != b.is_dir { b.is_dir.cmp(&a.is_dir) }
            else { a.name.to_lowercase().cmp(&b.name.to_lowercase()) }
        });
        self.path_input = self.cwd_display();
        self.selected_item = None;
        self.selected_items.clear();
        if keep_selection {
            self.selected_item = kept_item.and_then(|p| self.entries.iter().position(|e| e.path == p));
            self.selected_items = self.entries.iter().enumerate()
                .filter(|(_, e)| kept_multi.contains(&e.path)).map(|(i, _)| i).collect();
        }
    }
}
