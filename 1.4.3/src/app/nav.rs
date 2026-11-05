use crate::app::KeyFmApp;
use crate::app::ui_utils::loc_ok;
use crate::constants::{THIS_PC, RECYCLE, QUICK_ACCESS, VIRTUAL};
use crate::helpers::same_loc;
use std::path::{Path, PathBuf};

impl KeyFmApp {
    pub(crate) fn is_virtual_view(&self) -> bool {
        VIRTUAL.contains(&self.cwd.as_str())
    }

    pub(crate) fn cwd_display(&self) -> String {
        if self.is_virtual_view() {
            crate::app::ui_utils::location_label(&self.cwd)
        } else {
            self.cwd.clone()
        }
    }

    pub(crate) fn parent_location(&self) -> Option<String> {
        if self.cwd == THIS_PC || self.cwd == QUICK_ACCESS { return None; }
        if self.cwd == RECYCLE { return Some(THIS_PC.to_string()); }
        match Path::new(&self.cwd).parent() {
            Some(par) if !par.as_os_str().is_empty() => Some(par.to_string_lossy().to_string()),
            _ if cfg!(windows) => Some(THIS_PC.to_string()),
            _ => None,
        }
    }

    pub(crate) fn drive_root(&self) -> Option<String> {
        if self.is_virtual_view() || self.cwd.is_empty() { return None; }
        let mut cur = Path::new(&self.cwd);
        while let Some(p) = cur.parent() {
            if p.as_os_str().is_empty() { break; }
            cur = p;
        }
        let root = cur.to_string_lossy().to_string();
        if same_loc(&root, &self.cwd) { None } else { Some(root) }
    }

    pub(crate) fn cycle_quick_access(&self) -> Option<String> {
        let mut locs: Vec<String> = self.config.pins.clone();
        for (_, p) in &self.known_cache {
            locs.push(p.to_string_lossy().to_string());
        }
        let mut seen: Vec<String> = Vec::new();
        locs.retain(|l| {
            if seen.iter().any(|s| same_loc(s, l)) { false }
            else { seen.push(l.clone()); true }
        });
        if locs.is_empty() { return None; }
        let cur_pos = locs.iter().position(|l| same_loc(l, &self.cwd));
        let next_pos = match cur_pos { Some(p) => (p + 1) % locs.len(), None => 0 };
        Some(locs[next_pos].clone())
    }

    pub(crate) fn go_to(&mut self, new_loc: String) {
        self.cancel_search();
        self.search_query.clear();
        self.last_search_query.clear();
        if !same_loc(&new_loc, &self.cwd) {
            let cur = PathBuf::from(&self.cwd);
            if let Some(t) = self.tabs.get_mut(self.cur_tab) {
                t.back.push(cur);
                if t.back.len() > 200 { t.back.remove(0); }
                t.fwd.clear();
            }
        }
        self.set_cwd(new_loc);
    }

    pub(crate) fn pop_valid(&mut self, forward: bool) -> (Option<PathBuf>, Option<String>) {
        let mut skipped: Option<String> = None;
        loop {
            let popped = self.tabs.get_mut(self.cur_tab).and_then(|t| {
                if forward { t.fwd.pop() } else { t.back.pop() }
            });
            match popped {
                Some(c) => {
                    let s = c.to_string_lossy().to_string();
                    if loc_ok(&s) { return (Some(c), skipped); }
                    skipped = Some(s);
                }
                None => return (None, skipped),
            }
        }
    }

    pub(crate) fn go_back(&mut self) {
        self.cancel_search();
        self.search_query.clear();
        self.last_search_query.clear();
        let (prev, skipped) = self.pop_valid(false);
        if let Some(prev) = prev {
            let cur = PathBuf::from(&self.cwd);
            if let Some(t) = self.tabs.get_mut(self.cur_tab) { t.fwd.push(cur); }
            self.set_cwd(prev.to_string_lossy().to_string());
        }
        if let Some(s) = skipped {
            self.status_msg = format!("Folder no longer available: {}", s);
        }
    }

    pub(crate) fn go_forward(&mut self) {
        self.cancel_search();
        self.search_query.clear();
        self.last_search_query.clear();
        let (next, skipped) = self.pop_valid(true);
        if let Some(next) = next {
            let cur = PathBuf::from(&self.cwd);
            if let Some(t) = self.tabs.get_mut(self.cur_tab) { t.back.push(cur); }
            self.set_cwd(next.to_string_lossy().to_string());
        }
        if let Some(s) = skipped {
            self.status_msg = format!("Folder no longer available: {}", s);
        }
    }
}