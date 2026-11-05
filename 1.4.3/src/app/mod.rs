use eframe::egui;
use crate::config::Config;
use crate::theme::{LIGHT, DARK, Theme};
use crate::models::Tab;
use crate::icons::IconCache;
use crate::drives;
use crate::constants::{THIS_PC, QUICK_ACCESS};
use std::path::PathBuf;

mod types;
mod ui_utils;
mod search;
mod nav;
mod file_ops;
mod open_with;
mod open_with_picker;
mod update;
mod update_keyboard;
mod panels_tab_toolbar_status;
mod panels_sidebar;
mod panels_file_list;
mod dialogs_misc;
mod dialogs_open_with;

pub(crate) use types::ClipItem;
pub(crate) use search::SearchHit;
pub(crate) use open_with::OpenWithState;

pub struct KeyFmApp {
    config: Config,
    theme: Theme,
    cwd: String,
    entries: Vec<crate::models::Item>,
    selected_item: Option<usize>,
    search_query: String,
    path_input: String,
    tabs: Vec<Tab>,
    cur_tab: usize,
    clipboard: Vec<ClipItem>,
    clip_mode: String,
    active_dialog: Option<(String, String)>,
    lasso_start: Option<egui::Pos2>,
    selected_items: Vec<usize>,
    status_msg: String,
    confirm_empty_bin: bool,
    dialog_seen: bool,
    icons: IconCache,
    known_cache: Vec<(&'static str, PathBuf)>,
    drives_info_cache: Vec<(String, PathBuf, u64, u64)>,
    search_rx: Option<std::sync::mpsc::Receiver<SearchHit>>,
    search_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    search_threads: Option<Vec<std::thread::JoinHandle<()>>>,
    search_debounce: Option<std::time::Instant>,
    search_results: Vec<crate::models::Item>,
    search_running: bool,
    search_truncated: bool,
    last_search_query: String,
    show_shortcuts: bool,
    open_with: Option<OpenWithState>,
    sidebar_focus: bool,
    sidebar_idx: usize,
}

impl KeyFmApp {
    pub fn new(start: Option<String>) -> Self {
        let (mut config, config_notice) = crate::config::load_config_status();
        if config.sidebar_width < 120.0 { config.sidebar_width = crate::config::default_sidebar_width(); }
        if config.size_col_width < 60.0 { config.size_col_width = crate::config::default_size_col(); }
        if config.mod_col_width < 60.0 { config.mod_col_width = crate::config::default_mod_col(); }
        
        let theme = if config.dark { DARK } else { LIGHT };
        
        let start_dir = crate::helpers::get_start_dir(start.clone());
        let mut tabs = Vec::new();
        let mut cur_tab = 0;

        // Restore tabs if no specific directory was passed in the terminal
        if start.is_none() && !config.open_tabs.is_empty() {
            for t in &config.open_tabs {
                let p = PathBuf::from(t);
                // FIXED: Allow virtual paths like ::this-pc:: to be restored
                if crate::constants::VIRTUAL.contains(&t.as_str()) || p.is_dir() {
                    tabs.push(Tab::new(Some(p)));
                }
            }
            if !tabs.is_empty() {
                cur_tab = config.active_tab.min(tabs.len() - 1);
            }
        }
        
        // Fallback to default start dir if no tabs were restored
        if tabs.is_empty() {
            tabs.push(Tab::new(Some(start_dir)));
        }

        let cwd = tabs.get(cur_tab)
            .and_then(|t| t.cwd.as_ref())
            .cloned()
            .unwrap_or_else(|| PathBuf::from("."))
            .to_string_lossy()
            .to_string();
        let path_input = cwd.clone();

        let mut app = Self {
            config,
            show_shortcuts: false,
            theme,
            cwd,
            entries: Vec::new(),
            selected_item: None,
            search_query: String::new(),
            path_input,
            tabs,
            cur_tab,
            clipboard: Vec::new(),
            clip_mode: "copy".to_string(),
            active_dialog: None,
            lasso_start: None,
            selected_items: Vec::new(),
            status_msg: String::new(),
            confirm_empty_bin: false,
            dialog_seen: false,
            icons: IconCache::new(),
            known_cache: drives::known_folders(),
            drives_info_cache: drives::scan_drives_info(),
            search_rx: None,
            search_cancel: None,
            search_threads: None,
            search_debounce: None,
            search_results: Vec::new(),
            search_running: false,
            search_truncated: false,
            last_search_query: String::new(),
            open_with: None,
            sidebar_focus: false,
            sidebar_idx: 0,
        };
        app.refresh_dir();
        if let Some(n) = config_notice {
            app.status_msg = n;
        }
        app
    }

    pub(crate) fn save_current_tab(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.cur_tab) {
            tab.cwd = Some(std::path::PathBuf::from(&self.cwd));
            tab.search_query = self.search_query.clone();
            tab.focus = self.selected_item
                .and_then(|idx| self.entries.get(idx))
                .map(|item| item.path.clone());
        }
    }

    pub(crate) fn load_tab(&mut self, index: usize) {
        self.save_current_tab();
        self.load_tab_no_save(index);
        self.save_cfg(); // persist active_tab
    }

    pub(crate) fn load_tab_no_save(&mut self, index: usize) {
        self.cancel_search();
        self.search_query.clear();
        self.last_search_query.clear();
        if index < self.tabs.len() {
            let tab = &self.tabs[index];
            let new_cwd = tab.cwd.clone();
            let new_query = tab.search_query.clone();
            let new_focus = tab.focus.clone();
            if let Some(cwd) = new_cwd {
                self.cwd = cwd.to_string_lossy().to_string();
            }
            self.search_query = new_query;
            self.cur_tab = index;
            self.refresh_dir();
            if let Some(ref focus) = new_focus {
                self.selected_item = self.entries.iter().position(|item| &item.path == focus);
            }
        }
    }

    pub(crate) fn new_tab(&mut self) {
        self.save_current_tab();
        let new_t = Tab::new(Some(std::path::PathBuf::from(&self.cwd)));
        self.tabs.push(new_t);
        let idx = self.tabs.len() - 1;
        self.load_tab(idx);
        self.save_cfg(); // persist immediately
    }

    pub(crate) fn close_tab(&mut self) {
        self.close_tab_at(self.cur_tab);
    }

    pub(crate) fn close_tab_at(&mut self, idx: usize) {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() { return; }
        self.save_current_tab();
        self.tabs.remove(idx);
        if idx < self.cur_tab {
            self.cur_tab -= 1;
        } else if idx == self.cur_tab {
            if self.cur_tab >= self.tabs.len() { self.cur_tab = self.tabs.len() - 1; }
            let target = self.cur_tab;
            self.load_tab_no_save(target);
        }
        self.save_cfg(); // persist immediately
    }

    pub(crate) fn set_cwd(&mut self, new_cwd: String) {
        self.status_msg.clear();
        self.cwd = new_cwd;
        if let Some(t) = self.tabs.get_mut(self.cur_tab) {
            t.cwd = Some(std::path::PathBuf::from(&self.cwd));
        }
        self.refresh_dir();
    }

    pub(crate) fn sidebar_locations(&self) -> Vec<String> {
        let mut v: Vec<String> = vec![QUICK_ACCESS.to_string()];
        v.extend(self.config.pins.clone());
        for (_, p) in &self.known_cache {
            v.push(p.to_string_lossy().to_string());
        }
        v.push(THIS_PC.to_string());
        for (_, p, _, _) in &self.drives_info_cache {
            v.push(p.to_string_lossy().to_string());
        }
        v
    }

    pub(crate) fn modal_open(&self) -> bool {
        self.active_dialog.is_some() || self.confirm_empty_bin || self.show_shortcuts || self.open_with.is_some()
    }

    pub(crate) fn rendered_entries(&self) -> &[crate::models::Item] {
        if self.search_active() { &self.search_results } else { &self.entries }
    }

    pub(crate) fn search_active(&self) -> bool {
        self.search_query.chars().count() > 1 && (self.search_running || !self.search_results.is_empty())
    }

    pub(crate) fn is_visible(&self, idx: usize) -> bool {
        let f = self.search_query.to_lowercase();
        f.is_empty() || self.rendered_entries().get(idx).map_or(false, |e| e.name.to_lowercase().contains(&f))
    }

    pub(crate) fn visible_indices(&self) -> Vec<usize> {
        (0..self.rendered_entries().len()).filter(|&i| self.is_visible(i)).collect()
    }

    pub(crate) fn targets(&self) -> Vec<usize> {
        let mut t: Vec<usize> = if !self.selected_items.is_empty() {
            self.selected_items.clone()
        } else {
            self.selected_item.into_iter().collect()
        };
        t.sort();
        t.dedup();
        t.retain(|&i| self.is_visible(i));
        t
    }

    pub(crate) fn cancel_search(&mut self) {
        if let Some(c) = self.search_cancel.take() { c.store(true, std::sync::atomic::Ordering::Relaxed); }
        self.search_rx = None;
        if let Some(ts) = self.search_threads.take() {
            for t in ts { let _ = t.join(); }
        }
        self.search_running = false;
        self.search_results.clear();
        self.search_truncated = false;
        self.search_debounce = None;
    }

    pub(crate) fn save_cfg(&mut self) {
        self.save_current_tab(); // Ensure current path is synced to the tab
        self.config.open_tabs = self.tabs.iter()
            .filter_map(|t| t.cwd.as_ref())
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        self.config.active_tab = self.cur_tab;
        
        if !crate::config::save_config(&self.config) {
            self.status_msg = "Failed to save config".to_string();
        }
    }

    pub(crate) fn toggle_pin(&mut self, path: &str) {
        if let Some(pos) = self.config.pins.iter().position(|p| p == path) {
            self.config.pins.remove(pos);
        } else {
            self.config.pins.push(path.to_string());
        }
        self.save_cfg();
    }

    pub(crate) fn refresh_caches(&mut self) {
        self.known_cache = drives::known_folders();
        self.drives_info_cache = drives::scan_drives_info();
    }

    pub(crate) fn refresh_dir(&mut self) {
        self.reload(false);
    }

    pub(crate) fn refresh_keep_selection(&mut self) {
        self.refresh_caches();
        self.reload(true);
    }
}

// ADDED: Guarantees tabs are saved no matter how the window is closed
impl Drop for KeyFmApp {
    fn drop(&mut self) {
        self.save_cfg();
    }
}
