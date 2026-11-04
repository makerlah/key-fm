use eframe::egui;
use std::env;
use keyfm::config::Config;
use keyfm::theme::{LIGHT, DARK, Theme};
use keyfm::helpers::{get_start_dir, expand_and_validate_in, same_loc};
use keyfm::constants::{THIS_PC, RECYCLE, VIRTUAL, title_for, pc_title_for, bin_title_for};

use keyfm::icons::{IconCache, IconKind};
use keyfm::models::{Item, Tab};
use keyfm::drives;
use keyfm::helpers::{human_size, fmt_time};
use std::path::{Path, PathBuf};

/// Shared height of the menus row and the toolbar row so their
/// horizontal separators land on the same Y.
const TOPBAR_ROW_H: f32 = 28.0;
/// Horizontal padding inside each top-row / sidebar.
const PANEL_PAD_X: f32 = 8.0;
/// Space above the top row (inside the panel).
const PANEL_PAD_TOP: f32 = 4.0;

fn stroke_rect(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    painter.line_segment([rect.left_top(), rect.right_top()], stroke);
    painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
    painter.line_segment([rect.right_bottom(), rect.left_bottom()], stroke);
    painter.line_segment([rect.left_bottom(), rect.left_top()], stroke);
}

fn draw_icon(ui: &mut egui::Ui, ppp: f32, tex: Option<&egui::TextureHandle>, is_dir: bool) {
    let size = tex
        .map(|t| t.size_vec2() / ppp)
        .unwrap_or_else(|| egui::vec2(16.0, 16.0));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    match tex {
        Some(t) => {
            ui.painter().image(
                t.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None if is_dir => {
            ui.painter().rect_filled(
                rect.shrink2(egui::vec2(1.0, 3.0)),
                2.0,
                egui::Color32::from_rgb(0xf0, 0xc0, 0x4a),
            );
        }
        None => {}
    }
}

fn location_label(loc: &str) -> String {
    if loc == THIS_PC {
        "This PC".to_string()
    } else if loc == RECYCLE {
        "Recycle Bin".to_string()
    } else {
        Path::new(loc)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| loc.to_string())
    }
}

fn parse_virtual(raw: &str) -> Option<&'static str> {
    let l = raw.trim().to_lowercase();
    if l == THIS_PC || l == "this pc" || l == "this-pc" {
        Some(THIS_PC)
    } else if l == RECYCLE || l == "recycle bin" || l == "recycle-bin" {
        Some(RECYCLE)
    } else {
        None
    }
}

fn resolve_location(raw: &str, cwd: &str) -> Option<String> {
    if let Some(v) = parse_virtual(raw) {
        return Some(v.to_string());
    }
    let base = if VIRTUAL.contains(&cwd) {
        env::current_dir().unwrap_or_default()
    } else {
        PathBuf::from(cwd)
    };
    expand_and_validate_in(raw, &base).map(|p| p.to_string_lossy().to_string())
}

fn loc_ok(loc: &str) -> bool {
    VIRTUAL.contains(&loc) || (Path::new(loc).is_dir() && std::fs::read_dir(loc).is_ok())
}

#[derive(Clone)]
struct ClipItem {
    path: PathBuf,
    name: String,
}

enum UiAction {
    None,
    Copy,
    Cut,
    Paste,
    Rename(String),
    Delete,
    TogglePin(std::path::PathBuf),
    Open(std::path::PathBuf),
    OpenWith(std::path::PathBuf),
    SidebarPin(std::path::PathBuf),
    SidebarUnpin(std::path::PathBuf),
    NewFolder,
    NewFile,
    Refresh,
}

struct KeyFmApp {
    config: Config,
    theme: Theme,
    cwd: String,
    entries: Vec<Item>,
    selected_item: Option<usize>,
    filter: String,
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
}

impl KeyFmApp {
    fn new(start: Option<String>) -> Self {
        let (config, config_notice) = keyfm::config::load_config_status();
        let mut config = config;
        if config.sidebar_width < 120.0 {
            config.sidebar_width = keyfm::config::default_sidebar_width();
        }
        let theme = if config.dark { DARK } else { LIGHT };
        let start_dir = get_start_dir(start);
        let cwd = start_dir.to_string_lossy().to_string();
        let path_input = cwd.clone();
        let tab = Tab::new(Some(start_dir.clone()));
        let mut app = Self {
            config,
            theme,
            cwd,
            entries: Vec::new(),
            selected_item: None,
            filter: String::new(),
            path_input,
            tabs: vec![tab],
            cur_tab: 0,
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
        };
        app.refresh_dir();
        if let Some(n) = config_notice {
            app.status_msg = n;
        }
        app
    }

    fn save_current_tab(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.cur_tab) {
            tab.cwd = Some(std::path::PathBuf::from(&self.cwd));
            tab.filter = self.filter.clone();
            tab.focus = self.selected_item.and_then(|idx| self.entries.get(idx)).map(|item| item.path.clone());
        }
    }

    fn load_tab(&mut self, index: usize) {
        self.save_current_tab();
        self.load_tab_no_save(index);
    }

    fn load_tab_no_save(&mut self, index: usize) {
        if index < self.tabs.len() {
            let tab = &self.tabs[index];
            let new_cwd = tab.cwd.clone();
            let new_filter = tab.filter.clone();
            let new_focus = tab.focus.clone();

            if let Some(cwd) = new_cwd {
                self.cwd = cwd.to_string_lossy().to_string();
            }
            self.filter = new_filter;
            self.cur_tab = index;

            self.refresh_dir();

            if let Some(ref focus) = new_focus {
                self.selected_item = self.entries.iter().position(|item| &item.path == focus);
            }
        }
    }

    fn new_tab(&mut self) {
        self.save_current_tab();
        let new_t = Tab::new(Some(std::path::PathBuf::from(&self.cwd)));
        self.tabs.push(new_t);
        let idx = self.tabs.len() - 1;
        self.load_tab(idx);
    }

    fn close_tab(&mut self) {
        self.close_tab_at(self.cur_tab);
    }

    fn close_tab_at(&mut self, idx: usize) {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() {
            return;
        }
        self.save_current_tab();
        self.tabs.remove(idx);
        if idx < self.cur_tab {
            self.cur_tab -= 1;
        } else if idx == self.cur_tab {
            if self.cur_tab >= self.tabs.len() {
                self.cur_tab = self.tabs.len() - 1;
            }
            let target = self.cur_tab;
            self.load_tab_no_save(target);
        }
    }

    fn set_cwd(&mut self, new_cwd: String) {
        self.status_msg.clear();
        self.cwd = new_cwd;
        if let Some(t) = self.tabs.get_mut(self.cur_tab) {
            t.cwd = Some(std::path::PathBuf::from(&self.cwd));
        }
        self.refresh_dir();
    }

    fn is_virtual_view(&self) -> bool {
        VIRTUAL.contains(&self.cwd.as_str())
    }

    fn cwd_display(&self) -> String {
        if self.is_virtual_view() {
            location_label(&self.cwd)
        } else {
            self.cwd.clone()
        }
    }

    fn parent_location(&self) -> Option<String> {
        if self.cwd == THIS_PC {
            return None;
        }
        if self.cwd == RECYCLE {
            return Some(THIS_PC.to_string());
        }
        match Path::new(&self.cwd).parent() {
            Some(par) if !par.as_os_str().is_empty() => Some(par.to_string_lossy().to_string()),
            _ if cfg!(windows) => Some(THIS_PC.to_string()),
            _ => None,
        }
    }

    fn go_to(&mut self, new_loc: String) {
        if !same_loc(&new_loc, &self.cwd) {
            let cur = PathBuf::from(&self.cwd);
            if let Some(t) = self.tabs.get_mut(self.cur_tab) {
                t.back.push(cur);
                if t.back.len() > 200 {
                    t.back.remove(0);
                }
                t.fwd.clear();
            }
        }
        self.set_cwd(new_loc);
    }

    fn pop_valid(&mut self, forward: bool) -> (Option<PathBuf>, Option<String>) {
        let mut skipped: Option<String> = None;
        loop {
            let popped = self.tabs.get_mut(self.cur_tab).and_then(|t| {
                if forward { t.fwd.pop() } else { t.back.pop() }
            });
            match popped {
                Some(c) => {
                    let s = c.to_string_lossy().to_string();
                    if loc_ok(&s) {
                        return (Some(c), skipped);
                    }
                    skipped = Some(s);
                }
                None => return (None, skipped),
            }
        }
    }

    fn go_back(&mut self) {
        let (prev, skipped) = self.pop_valid(false);
        if let Some(prev) = prev {
            let cur = PathBuf::from(&self.cwd);
            if let Some(t) = self.tabs.get_mut(self.cur_tab) {
                t.fwd.push(cur);
            }
            self.set_cwd(prev.to_string_lossy().to_string());
        }
        if let Some(s) = skipped {
            self.status_msg = format!("Folder no longer available: {}", s);
        }
    }

    fn go_forward(&mut self) {
        let (next, skipped) = self.pop_valid(true);
        if let Some(next) = next {
            let cur = PathBuf::from(&self.cwd);
            if let Some(t) = self.tabs.get_mut(self.cur_tab) {
                t.back.push(cur);
            }
            self.set_cwd(next.to_string_lossy().to_string());
        }
        if let Some(s) = skipped {
            self.status_msg = format!("Folder no longer available: {}", s);
        }
    }

    fn load_this_pc(&mut self) {
        for (label, mount, free, total) in drives::scan_drives_info() {
            self.entries.push(Item {
                name: label,
                path: mount,
                is_dir: true,
                size: free,
                mtime: None,
                hidden: false,
                extra: String::new(),
                orig: None,
                info: String::new(),
                total,
            });
        }
    }

    fn load_recycle(&mut self) {
        if !cfg!(windows) {
            self.status_msg = "The Recycle Bin view is only available on Windows".to_string();
            return;
        }
        for e in keyfm::trash::list_recycle_bin() {
            self.entries.push(Item {
                name: e.name,
                path: e.path,
                is_dir: e.is_dir,
                size: e.size,
                mtime: e.deleted,
                hidden: false,
                extra: String::new(),
                orig: Some(PathBuf::from(e.original)),
                info: String::new(),
                total: 0,
            });
        }
    }

    fn modal_open(&self) -> bool {
        self.active_dialog.is_some() || self.confirm_empty_bin
    }

    fn is_visible(&self, idx: usize) -> bool {
        let f = self.filter.to_lowercase();
        f.is_empty()
            || self.entries.get(idx).map_or(false, |e| e.name.to_lowercase().contains(&f))
    }

    fn visible_indices(&self) -> Vec<usize> {
        (0..self.entries.len()).filter(|&i| self.is_visible(i)).collect()
    }

    fn targets(&self) -> Vec<usize> {
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

    fn copy_or_cut(&mut self, mode: &str) {
        if self.cwd == THIS_PC || (self.cwd == RECYCLE && mode == "cut") {
            self.status_msg = "Not available in this view".to_string();
            return;
        }
        let t = self.targets();
        if !t.is_empty() {
            self.clipboard.clear();
            for idx in t {
                if let Some(item) = self.entries.get(idx) {
                    let name = item
                        .orig
                        .as_ref()
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

    fn paste(&mut self) {
        if self.is_virtual_view() {
            self.status_msg = "Cannot paste into this view".to_string();
            return;
        }
        if self.clipboard.is_empty() {
            return;
        }
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
            let same_folder = src
                .parent()
                .map_or(false, |p| same_loc(&p.to_string_lossy(), &self.cwd));
            if cut && same_folder {
                continue;
            }
            let dest = keyfm::helpers::unique_path(&dest_dir, &item.name, meta.is_dir());
            let res = if cut {
                keyfm::helpers::move_path(src, &dest)
            } else {
                keyfm::helpers::copy_any(src, &dest)
            };
            match res {
                Ok(()) => done += 1,
                Err(e) => {
                    first_err.get_or_insert(format!("{}: {}", item.name, e));
                    failed.push(item.clone());
                }
            }
        }

        if cut {
            self.clipboard = failed.clone();
        }
        self.refresh_dir();
        let nfail = failed.len() + missing;
        self.status_msg = match first_err {
            Some(err) if nfail > 0 => format!("Pasted {}, {} failed ({})", done, nfail, err),
            _ => String::new(),
        };
    }

    fn delete_selected(&mut self, permanent: bool) {
        if self.is_virtual_view() {
            self.status_msg = "Cannot delete from this view".to_string();
            return;
        }
        let paths: Vec<PathBuf> = self
            .targets()
            .into_iter()
            .filter_map(|i| self.entries.get(i).map(|e| e.path.clone()))
            .collect();
        if paths.is_empty() {
            return;
        }

        let mut failed: Vec<PathBuf> = Vec::new();
        let mut first_err: Option<String> = None;
        for (i, path) in paths.iter().enumerate() {
            let res = if permanent {
                keyfm::helpers::delete_permanently(path)
            } else {
                keyfm::trash::move_to_trash(path)
            };
            if let Err(e) = res {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    failed.extend(paths[i..].iter().cloned());
                    first_err.get_or_insert("cancelled".to_string());
                    break;
                }
                let what = if permanent { "Delete" } else { "Trash" };
                first_err.get_or_insert(format!("{} failed for \"{}\": {}", what, path.display(), e));
                failed.push(path.clone());
            }
        }

        self.refresh_dir();
        if !failed.is_empty() {
            let sel: Vec<usize> = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| failed.contains(&e.path))
                .map(|(i, _)| i)
                .collect();
            self.selected_item = sel.first().copied();
            self.selected_items = sel;
            self.status_msg = format!(
                "{} of {} item(s) not deleted: {}",
                failed.len(),
                paths.len(),
                first_err.unwrap_or_default()
            );
        }
    }

    fn save_cfg(&mut self) {
        if !keyfm::config::save_config(&self.config) {
            self.status_msg = "Failed to save config".to_string();
        }
    }

    fn toggle_pin(&mut self, path: &str) {
        if let Some(pos) = self.config.pins.iter().position(|p| p == path) {
            self.config.pins.remove(pos);
        } else {
            self.config.pins.push(path.to_string());
        }
        self.save_cfg();
    }

    fn refresh_caches(&mut self) {
        self.known_cache = drives::known_folders();
        self.drives_info_cache = drives::scan_drives_info();
    }

    fn refresh_dir(&mut self) {
        self.reload(false);
    }

    fn refresh_keep_selection(&mut self) {
        self.refresh_caches();
        self.reload(true);
    }

    fn reload(&mut self, keep_selection: bool) {
        let kept_item: Option<PathBuf> = if keep_selection {
            self.selected_item.and_then(|i| self.entries.get(i)).map(|e| e.path.clone())
        } else {
            None
        };
        let kept_multi: Vec<PathBuf> = if keep_selection {
            self.selected_items
                .iter()
                .filter_map(|&i| self.entries.get(i))
                .map(|e| e.path.clone())
                .collect()
        } else {
            Vec::new()
        };

        self.entries.clear();
        if self.cwd == THIS_PC {
            self.load_this_pc();
        } else if self.cwd == RECYCLE {
            self.load_recycle();
        }
        let is_virtual = self.is_virtual_view();
        let path = Path::new(&self.cwd);
        let read_dir_res = if is_virtual { None } else { Some(std::fs::read_dir(path)) };
        if let Some(Err(e)) = &read_dir_res {
            self.status_msg = format!("Cannot read {}: {}", self.cwd, e);
        }
        if let Some(Ok(read_dir)) = read_dir_res {
            for entry in read_dir.flatten() {
                let path = entry.path();
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let metadata = entry.metadata().ok();
                let hidden = name.starts_with('.')
                    || keyfm::helpers::is_windows_hidden(metadata.as_ref());
                if !self.config.show_hidden && hidden {
                    continue;
                }
                let is_link = metadata.as_ref().map_or(false, |m| m.file_type().is_symlink());
                let metadata = if is_link {
                    std::fs::metadata(&path).ok().or(metadata)
                } else {
                    metadata
                };
                let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = metadata.as_ref().map(|m| if is_dir { 0 } else { m.len() }).unwrap_or(0);
                let mtime = metadata.as_ref().and_then(|m| m.modified().ok()).and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64())
                });

                self.entries.push(Item {
                    name,
                    path,
                    is_dir,
                    size,
                    mtime,
                    hidden,
                    extra: String::new(),
                    orig: None,
                    info: String::new(),
                    total: 0,
                });
            }
        }
        self.entries.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                b.is_dir.cmp(&a.is_dir)
            } else {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            }
        });
        self.path_input = self.cwd_display();
        self.selected_item = None;
        self.selected_items.clear();

        if keep_selection {
            self.selected_item = kept_item
                .and_then(|p| self.entries.iter().position(|e| e.path == p));
            self.selected_items = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| kept_multi.contains(&e.path))
                .map(|(i, _)| i)
                .collect();
        }
    }
}

impl eframe::App for KeyFmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut navigate_to: Option<String> = None;
        let mut pending_action = UiAction::None;
        let mut nav_back = false;
        let mut nav_forward = false;
        self.icons.begin_frame();

        // ---- Theme (unchanged) ----
        let mut visuals = if self.config.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.window_fill = self.theme.bg;
        visuals.panel_fill = self.theme.bg;
        visuals.override_text_color = Some(self.theme.fg);
        visuals.widgets.noninteractive.bg_fill = self.theme.bg;
        visuals.widgets.noninteractive.fg_stroke.color = self.theme.fg;
        visuals.widgets.inactive.bg_fill = self.theme.field;
        visuals.widgets.inactive.fg_stroke.color = self.theme.fg;
        visuals.widgets.hovered.bg_fill = self.theme.sel_bg;
        visuals.widgets.hovered.fg_stroke.color = self.theme.fg;
        visuals.widgets.active.bg_fill = self.theme.sel_bg;
        visuals.widgets.active.fg_stroke.color = self.theme.fg;
        visuals.selection.bg_fill = self.theme.sel_bg;
        visuals.selection.stroke.color = self.theme.fg;
        ctx.set_visuals(visuals);

        // ---- Panel 1: tab bar (full width, top) ----
        egui::TopBottomPanel::top("tab_bar")
            .frame(egui::Frame::default().fill(self.theme.bg))
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(PANEL_PAD_X);
                    ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);

                    let mut switch_to_tab: Option<usize> = None;
                    let mut close_tab_idx: Option<usize> = None;
                    let mut new_tab_clicked = false;
                    let tab_count = self.tabs.len();
                    let theme = self.theme;
                    let cur_tab = self.cur_tab;

                    let tab_height = 30.0;
                    let padding = 12.0;
                    let close_w = 22.0;
                    let plus_w = 28.0;

                    for (i, tab) in self.tabs.iter().enumerate() {
                        let title = tab
                            .cwd
                            .as_ref()
                            .map(|p| location_label(&p.to_string_lossy()))
                            .unwrap_or_else(|| "Tab".to_string());
                        let is_active = cur_tab == i;
                        let display_text = format!("\u{1F4C1}  {}", title);
                        let font_id = egui::FontId::proportional(13.0);
                        let text_color = if is_active { theme.fg } else { theme.dim };
                        let galley = ui.painter().layout_no_wrap(display_text, font_id, text_color);
                        let text_w = galley.size().x.min(220.0);
                        let tab_width = (text_w + padding * 2.0 + close_w).max(150.0);
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(tab_width, tab_height),
                            egui::Sense::click(),
                        );
                        let rounding = egui::Rounding { nw: 8.0, ne: 8.0, sw: 0.0, se: 0.0 };
                        let bg = if is_active { theme.tab_sel } else { theme.tab };
                        ui.painter().rect_filled(rect, rounding, bg);
                        if response.hovered() && !is_active {
                            ui.painter().rect_filled(rect, rounding, theme.hover);
                        }
                        let text_pos = egui::pos2(
                            rect.left() + padding,
                            rect.center().y - galley.size().y / 2.0,
                        );
                        ui.painter().galley(text_pos, galley, text_color);
                        if tab_count > 1 {
                            let close_rect = egui::Rect::from_center_size(
                                egui::pos2(rect.right() - close_w / 2.0 - 3.0, rect.center().y),
                                egui::vec2(14.0, 14.0),
                            );
                            let close_hovered = response.hovered()
                                && ui
                                    .input(|i| i.pointer.hover_pos())
                                    .map_or(false, |p| close_rect.contains(p));
                            let close_color = if close_hovered { theme.danger } else { theme.dim };
                            let stroke = egui::Stroke::new(1.5_f32, close_color);
                            let c = close_rect.center();
                            let h = 4.0;
                            ui.painter().line_segment(
                                [egui::pos2(c.x - h, c.y - h), egui::pos2(c.x + h, c.y + h)],
                                stroke,
                            );
                            ui.painter().line_segment(
                                [egui::pos2(c.x + h, c.y - h), egui::pos2(c.x - h, c.y + h)],
                                stroke,
                            );
                            if response.clicked() {
                                let pos = response.interact_pointer_pos().unwrap_or(rect.center());
                                if close_rect.contains(pos) {
                                    close_tab_idx = Some(i);
                                } else if !is_active {
                                    switch_to_tab = Some(i);
                                }
                            }
                        } else if response.clicked() && !is_active {
                            switch_to_tab = Some(i);
                        }
                        if let Some(p) = tab.cwd.as_ref() {
                            response.clone().on_hover_text(p.to_string_lossy().to_string());
                        }
                    }

                    ui.add_space(4.0);
                    let (plus_rect, plus_response) = ui.allocate_exact_size(
                        egui::vec2(plus_w, tab_height),
                        egui::Sense::click(),
                    );
                    if plus_response.hovered() {
                        ui.painter().rect_filled(plus_rect, 6.0, theme.hover);
                    }
                    let plus_color = if plus_response.hovered() { theme.fg } else { theme.dim };
                    let c = plus_rect.center();
                    let h = 5.0;
                    let stroke = egui::Stroke::new(1.5_f32, plus_color);
                    ui.painter()
                        .line_segment([egui::pos2(c.x - h, c.y), egui::pos2(c.x + h, c.y)], stroke);
                    ui.painter()
                        .line_segment([egui::pos2(c.x, c.y - h), egui::pos2(c.x, c.y + h)], stroke);
                    if plus_response.clicked() {
                        new_tab_clicked = true;
                    }

                    if let Some(i) = close_tab_idx {
                        self.close_tab_at(i);
                    }
                    if let Some(tab_idx) = switch_to_tab {
                        if tab_idx != self.cur_tab {
                            self.load_tab(tab_idx);
                        }
                    }
                    if new_tab_clicked {
                        self.new_tab();
                    }
                });
                ui.add_space(4.0);
            });

        // ---- Panel 2: sidebar (left) — menus at top, then quick access ----
        let sidebar_inner = egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(self.config.sidebar_width)
            .min_width(120.0)
            .max_width(600.0)
            .frame(egui::Frame::default().fill(self.theme.bg))
            .show(ctx, |ui| {
                let panel_rect = ui.max_rect();

                // ----- Menu row -----
                ui.add_space(PANEL_PAD_TOP);
                let menu_row_top = ui.cursor().top();
                ui.horizontal(|ui| {
                    ui.set_min_height(TOPBAR_ROW_H);
                    ui.set_max_height(TOPBAR_ROW_H);
                    ui.add_space(PANEL_PAD_X);
                    ui.menu_button("File", |ui| {
                        if ui.button("New Tab").clicked() {
                            self.new_tab();
                            ui.close_menu();
                        }
                        if ui.button("Close Tab").clicked() {
                            self.close_tab();
                            ui.close_menu();
                        }
                        if ui.button("New Folder").clicked() {
                            self.active_dialog =
                                Some(("NewFolder".to_string(), "".to_string()));
                            ui.close_menu();
                        }
                        if ui.button("New File").clicked() {
                            self.active_dialog =
                                Some(("NewFile".to_string(), "".to_string()));
                            ui.close_menu();
                        }
                        if ui.button("Empty Recycle Bin").clicked() {
                            self.confirm_empty_bin = true;
                            ui.close_menu();
                        }
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button("Edit", |ui| {
                        if ui.button("Copy").clicked() {
                            self.copy_or_cut("copy");
                            ui.close_menu();
                        }
                        if ui.button("Cut").clicked() {
                            self.copy_or_cut("cut");
                            ui.close_menu();
                        }
                        if ui.button("Paste").clicked() {
                            self.paste();
                            ui.close_menu();
                        }
                        if ui.button("Rename").clicked() {
                            if let Some(idx) = self.selected_item {
                                if let Some(item) = self.entries.get(idx) {
                                    self.active_dialog =
                                        Some(("Rename".to_string(), item.name.clone()));
                                }
                            }
                            ui.close_menu();
                        }
                        if ui.button("Delete (Recycle)").clicked() {
                            self.delete_selected(false);
                            ui.close_menu();
                        }
                        if ui.button("Delete Permanently").clicked() {
                            self.delete_selected(true);
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("View", |ui| {
                        if ui.checkbox(&mut self.config.dark, "Dark mode (Ctrl+Shift+D)").changed() {
                            self.theme = if self.config.dark { DARK } else { LIGHT };
                            self.save_cfg();
                        }
                        if ui.checkbox(&mut self.config.show_hidden, "Show hidden files").changed() {
                            self.save_cfg();
                            self.refresh_keep_selection();
                        }
                        ui.separator();
                        if ui.button("Refresh").clicked() {
                            self.refresh_keep_selection();
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Go", |ui| {
                        if ui.button("Back").clicked() {
                            nav_back = true;
                            ui.close_menu();
                        }
                        if ui.button("Forward").clicked() {
                            nav_forward = true;
                            ui.close_menu();
                        }
                        if ui.button("Up").clicked() {
                            navigate_to = self.parent_location();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("This PC").clicked() {
                            navigate_to = Some(THIS_PC.to_string());
                            ui.close_menu();
                        }
                        if cfg!(windows) && ui.button("Recycle Bin").clicked() {
                            navigate_to = Some(RECYCLE.to_string());
                            ui.close_menu();
                        }
                        ui.separator();
                        for (label, path) in &self.known_cache {
                            let p = path.to_string_lossy().to_string();
                            if VIRTUAL.contains(&p.as_str()) {
                                continue;
                            }
                            if ui.button(*label).clicked() {
                                navigate_to = Some(p);
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        for (name, path, _, _) in &self.drives_info_cache {
                            if ui.button(name).clicked() {
                                navigate_to = Some(path.to_string_lossy().to_string());
                                ui.close_menu();
                            }
                        }
                    });
                });
                let menu_row_bottom = menu_row_top + TOPBAR_ROW_H;

                // ----- Separators (horizontal under menus, vertical on right edge) -----
                let sep = egui::Stroke::new(1.0, self.theme.border);
                ui.painter().line_segment(
                    [
                        egui::pos2(panel_rect.left(), menu_row_bottom),
                        egui::pos2(panel_rect.right(), menu_row_bottom),
                    ],
                    sep,
                );
                ui.painter().line_segment(
                    [
                        egui::pos2(panel_rect.right() - 0.5, panel_rect.top()),
                        egui::pos2(panel_rect.right() - 0.5, panel_rect.bottom()),
                    ],
                    sep,
                );

                // ----- Sidebar content (unchanged) -----
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Quick access").strong().color(self.theme.dim));

                if !self.config.pins.is_empty() {
                    for pin in &self.config.pins {
                        let label = std::path::Path::new(pin)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| pin.clone());
                        let pin_path = std::path::PathBuf::from(pin);
                        let is_selected = same_loc(&self.cwd, pin);
                        let resp = ui.selectable_label(is_selected, format!("📌 {}", label));
                        if resp.clicked() {
                            navigate_to = Some(pin.clone());
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Open").clicked() {
                                navigate_to = Some(pin_path.to_string_lossy().to_string());
                                ui.close_menu();
                            }
                            if ui.button("Unpin").clicked() {
                                pending_action = UiAction::SidebarUnpin(pin_path.clone());
                                ui.close_menu();
                            }
                        });
                    }
                    ui.add_space(4.0);
                }

                let known_cache = self.known_cache.clone();
                for (label, path) in &known_cache {
                    let path_str = path.to_string_lossy().to_string();
                    let is_selected = same_loc(&self.cwd, &path_str);
                    let resp = ui.selectable_label(is_selected, *label);
                    if resp.clicked() {
                        navigate_to = Some(path_str.clone());
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Open").clicked() {
                            navigate_to = Some(path_str.clone());
                            ui.close_menu();
                        }
                        if !VIRTUAL.contains(&path_str.as_str()) {
                            if ui.button("Pin").clicked() {
                                pending_action =
                                    UiAction::SidebarPin(std::path::PathBuf::from(&path_str));
                                ui.close_menu();
                            }
                            if ui.button("Unpin").clicked() {
                                pending_action =
                                    UiAction::SidebarUnpin(std::path::PathBuf::from(&path_str));
                                ui.close_menu();
                            }
                        }
                    });
                }

                ui.add_space(8.0);
                let is_pc_selected = self.cwd == THIS_PC;
                if ui
                    .selectable_label(is_pc_selected, egui::RichText::new("This PC").strong())
                    .clicked()
                {
                    navigate_to = Some(THIS_PC.to_string());
                }

                let drives_info_cache = self.drives_info_cache.clone();
                let ppp = ctx.pixels_per_point();
                for (label, path, free, total) in &drives_info_cache {
                    let path_str = path.to_string_lossy().to_string();
                    let is_selected = same_loc(&self.cwd, &path_str);
                    let usage = if *total > 0 {
                        ((*total - *free) as f64 / *total as f64) * 100.0
                    } else {
                        0.0
                    };
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        let tex = self.icons.get(ctx, IconKind::Drive(path));
                        draw_icon(ui, ppp, tex.as_ref(), false);
                        let text = format!("{} {:.1}%", label, usage);
                        if ui.selectable_label(is_selected, text).clicked() {
                            navigate_to = Some(path_str.clone());
                        }
                    });
                }
            });

        let sidebar_width = sidebar_inner.response.rect.width();
        if (sidebar_width - self.config.sidebar_width).abs() > 1.0 {
            self.config.sidebar_width = sidebar_width;
            self.save_cfg();
        }

        // ---- Panel 3: toolbar (top of the right area) ----
        let can_back = self.tabs.get(self.cur_tab).map_or(false, |t| !t.back.is_empty());
        let can_fwd = self.tabs.get(self.cur_tab).map_or(false, |t| !t.fwd.is_empty());
        let can_up = self.parent_location().is_some();
        egui::TopBottomPanel::top("toolbar")
            .frame(egui::Frame::default().fill(self.theme.bg))
            .show(ctx, |ui| {
                let panel_rect = ui.max_rect();
                ui.add_space(PANEL_PAD_TOP);
                let row_top = ui.cursor().top();
                ui.horizontal(|ui| {
                    ui.set_min_height(TOPBAR_ROW_H);
                    ui.set_max_height(TOPBAR_ROW_H);
                    ui.add_space(PANEL_PAD_X);

                    if ui.add_enabled(can_back, egui::Button::new("Back")).clicked() {
                        nav_back = true;
                    }
                    if ui.add_enabled(can_fwd, egui::Button::new("Forward")).clicked() {
                        nav_forward = true;
                    }
                    if ui.add_enabled(can_up, egui::Button::new("Up")).clicked() {
                        navigate_to = self.parent_location();
                    }
                    ui.label("Path:");
                    let resp = ui.text_edit_singleline(&mut self.path_input);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        navigate_to = Some(self.path_input.clone());
                    }
                    ui.label("Filter:");
                    ui.text_edit_singleline(&mut self.filter);
                });
                let row_bottom = row_top + TOPBAR_ROW_H;

                let sep = egui::Stroke::new(1.0, self.theme.border);
                ui.painter().line_segment(
                    [
                        egui::pos2(panel_rect.left(), row_bottom),
                        egui::pos2(panel_rect.right(), row_bottom),
                    ],
                    sep,
                );
            });

        // ---- Keyboard handling (unchanged) ----
        if !ctx.wants_keyboard_input() && !self.modal_open() {
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self.selected_item.and_then(|s| vis.iter().position(|&v| v == s));
                    self.selected_item = Some(match pos {
                        Some(p) => vis[(p + 1).min(vis.len() - 1)],
                        None => vis[0],
                    });
                    self.selected_items.clear();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self.selected_item.and_then(|s| vis.iter().position(|&v| v == s));
                    self.selected_item = Some(match pos {
                        Some(p) => vis[p.saturating_sub(1)],
                        None => vis[0],
                    });
                    self.selected_items.clear();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.entries.get(idx) {
                        if item.is_dir {
                            navigate_to = Some(item.path.to_string_lossy().to_string());
                        }
                    }
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Backspace) && !i.modifiers.shift) {
                let has_history =
                    self.tabs.get(self.cur_tab).map_or(false, |t| !t.back.is_empty());
                if has_history {
                    nav_back = true;
                } else {
                    navigate_to = self.parent_location();
                }
            }
            if ctx.input(|i| {
                (i.key_pressed(egui::Key::Backspace) && i.modifiers.shift)
                    || (i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp))
            }) {
                navigate_to = self.parent_location();
            }
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) {
                nav_back = true;
            }
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) {
                nav_forward = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
                self.refresh_keep_selection();
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::C)) {
                self.copy_or_cut("copy");
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::X)) {
                self.copy_or_cut("cut");
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::V)) {
                self.paste();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                self.delete_selected(false);
            }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::D)) {
                self.config.dark = !self.config.dark;
                self.theme = if self.config.dark { DARK } else { LIGHT };
                self.save_cfg();
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::T)) {
                self.new_tab();
            }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Tab)) {
                if self.tabs.len() > 1 {
                    let prev = if self.cur_tab == 0 {
                        self.tabs.len() - 1
                    } else {
                        self.cur_tab - 1
                    };
                    self.load_tab(prev);
                }
            } else if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Tab)) {
                if self.tabs.len() > 1 {
                    let next = (self.cur_tab + 1) % self.tabs.len();
                    self.load_tab(next);
                }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::W)) {
                if self.tabs.len() > 1 {
                    self.close_tab();
                }
            }
        }

        // ---- Panel 4: bottom status bar (unchanged) ----
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let total = self.entries.len();
                let shown = if self.filter.is_empty() { total } else { self.visible_indices().len() };
                if shown == total {
                    ui.label(format!("{} items", total));
                } else {
                    ui.label(format!("{} of {} items", shown, total));
                }
                if !self.status_msg.is_empty() {
                    ui.separator();
                    ui.label(&self.status_msg);
                }
            });
        });

        // ---- Panel 5: central file list (unchanged) ----
        egui::CentralPanel::default().show(ctx, |ui| {
            let pointer_pos = ui.input(|i| i.pointer.interact_pos());
            let primary_down = ui.input(|i| i.pointer.primary_down());
            if ui.input(|i| i.pointer.primary_pressed()) {
                let on_panel = pointer_pos
                    .map(|p| ui.max_rect().contains(p) && ctx.layer_id_at(p) == Some(ui.layer_id()))
                    .unwrap_or(false);
                if on_panel {
                    self.lasso_start = pointer_pos;
                    self.selected_items.clear();
                } else {
                    self.lasso_start = None;
                }
            }
            let lasso_rect: Option<egui::Rect> = if primary_down {
                match (self.lasso_start, pointer_pos) {
                    (Some(start), Some(cur)) if start.distance(cur) > 4.0 => {
                        Some(egui::Rect::from_two_pos(start, cur))
                    }
                    _ => None,
                }
            } else {
                None
            };
            if lasso_rect.is_some() {
                self.selected_items.clear();
                self.selected_item = None;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    let filter = self.filter.to_lowercase();
                    let is_pc = self.cwd == THIS_PC;
                    let in_bin = self.cwd == RECYCLE;
                    let ppp = ctx.pixels_per_point();
                    let row_h = if is_pc { 36.0 } else { 18.0 };
                    let size_w = if is_pc { 320.0 } else { 100.0 };
                    let mod_w = if is_pc { 10.0 } else { 150.0 };
                    let size_title = if is_pc {
                        pc_title_for("size").unwrap_or("")
                    } else {
                        title_for("size").unwrap_or("Size")
                    };
                    let mod_title = if is_pc {
                        pc_title_for("modified").unwrap_or("")
                    } else if in_bin {
                        bin_title_for("modified").unwrap_or("Date deleted")
                    } else {
                        title_for("modified").unwrap_or("Modified")
                    };
                    egui_extras::TableBuilder::new(ui)
                        .vscroll(false)
                        .column(egui_extras::Column::remainder())
                        .column(egui_extras::Column::exact(size_w))
                        .column(egui_extras::Column::exact(mod_w))
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.strong("Name");
                            });
                            header.col(|ui| {
                                ui.strong(size_title);
                            });
                            header.col(|ui| {
                                ui.strong(mod_title);
                            });
                        })
                        .body(|mut body| {
                            for (idx, item) in self.entries.iter().enumerate() {
                                if !filter.is_empty() && !item.name.to_lowercase().contains(&filter) {
                                    continue;
                                }
                                let name_str = item.name.clone();
                                let size_str = if is_pc {
                                    human_size(item.size)
                                } else if item.is_dir {
                                    String::new()
                                } else {
                                    human_size(item.size)
                                };
                                let time_str = fmt_time(item.mtime);
                                let is_selected = self.selected_item == Some(idx);
                                let is_multi_selected = self.selected_items.contains(&idx);
                                let is_dir = item.is_dir;
                                let item_path = item.path.clone();
                                let item_name = item.name.clone();
                                let item_orig = item.orig.clone();
                                let item_total = item.total;
                                let item_size = item.size;
                                let icon_name = item_orig
                                    .as_ref()
                                    .and_then(|o| o.file_name())
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| item_name.clone());
                                let icon_kind = if is_pc {
                                    IconKind::Drive(&item_path)
                                } else if is_dir {
                                    IconKind::Folder
                                } else {
                                    IconKind::File { name: &icon_name, path: &item_path }
                                };
                                let pin_str = item.path.to_string_lossy().to_string();
                                let is_pinned = self.config.pins.iter().any(|p| p == &pin_str);

                                body.row(row_h, |mut row| {
                                    row.col(|ui| {
                                        let response = ui.interact(
                                            ui.max_rect(),
                                            ui.id().with(("row_sel", idx)),
                                            egui::Sense::click(),
                                        );
                                        if let Some(lr) = lasso_rect {
                                            if lr.intersects(response.rect) {
                                                if !self.selected_items.contains(&idx) {
                                                    self.selected_items.push(idx);
                                                }
                                                if self.selected_item.is_none() {
                                                    self.selected_item = Some(idx);
                                                }
                                            }
                                        }
                                        if is_selected || is_multi_selected {
                                            ui.painter().rect_filled(
                                                response.rect,
                                                0.0,
                                                self.theme.sel_bg,
                                            );
                                        } else if response.hovered() {
                                            stroke_rect(
                                                ui.painter(),
                                                response.rect,
                                                egui::Stroke::new(1.0_f32, self.theme.accent),
                                            );
                                        }
                                        if response.clicked() {
                                            self.selected_item = Some(idx);
                                        }
                                        if response.secondary_clicked() {
                                            if !self.selected_items.contains(&idx) {
                                                self.selected_items.clear();
                                            }
                                            self.selected_item = Some(idx);
                                        }
                                        if response.double_clicked() {
                                            if is_dir {
                                                navigate_to = Some(
                                                    item_path.to_string_lossy().to_string(),
                                                );
                                            } else {
                                                pending_action = UiAction::Open(item_path.clone());
                                            }
                                        }

                                        let visible = ui.is_rect_visible(ui.max_rect());
                                        ui.horizontal(|ui| {
                                            let tex = if visible {
                                                self.icons.get(ctx, icon_kind)
                                            } else {
                                                None
                                            };
                                            draw_icon(ui, ppp, tex.as_ref(), is_dir);
                                            ui.label(name_str);
                                            if let Some(o) = &item_orig {
                                                ui.weak(format!("  (from {})", o.display()));
                                            }
                                        });

                                        response.context_menu(|ui| {
                                            if ui.button("Open").clicked() {
                                                pending_action = UiAction::Open(item_path.clone());
                                                ui.close_menu();
                                            }
                                            if !is_dir
                                                && ui.button("Open With (default app)").clicked()
                                            {
                                                pending_action = UiAction::OpenWith(item_path.clone());
                                                ui.close_menu();
                                            }
                                            ui.separator();
                                            if ui.button("Copy").clicked() {
                                                pending_action = UiAction::Copy;
                                                ui.close_menu();
                                            }
                                            if ui.button("Cut").clicked() {
                                                pending_action = UiAction::Cut;
                                                ui.close_menu();
                                            }
                                            if ui.button("Paste").clicked() {
                                                pending_action = UiAction::Paste;
                                                ui.close_menu();
                                            }
                                            ui.separator();
                                            if ui.button("Rename").clicked() {
                                                pending_action = UiAction::Rename(item_name.clone());
                                                ui.close_menu();
                                            }
                                            if ui.button("Delete").clicked() {
                                                pending_action = UiAction::Delete;
                                                ui.close_menu();
                                            }
                                            if is_dir && !in_bin {
                                                ui.separator();
                                                let label = if is_pinned {
                                                    "Unpin from Quick Access"
                                                } else {
                                                    "Pin to Quick Access"
                                                };
                                                if ui.button(label).clicked() {
                                                    pending_action =
                                                        UiAction::TogglePin(item_path.clone());
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                    });

                                    row.col(|ui| {
                                        if is_selected || is_multi_selected {
                                            ui.painter().rect_filled(ui.max_rect(), 0.0, self.theme.sel_bg);
                                        }
                                        if is_pc && item_total > 0 {
                                            let used = item_total.saturating_sub(item_size);
                                            let frac =
                                                (used as f32 / item_total as f32).clamp(0.0, 1.0);
                                            let fill = if frac > 0.9 {
                                                self.theme.danger
                                            } else {
                                                self.theme.accent
                                            };
                                            let track = self.theme.border;
                                            ui.vertical(|ui| {
                                                let w = (ui.available_width() - 12.0).max(60.0);
                                                let (rect, _) = ui.allocate_exact_size(
                                                    egui::vec2(w, 8.0),
                                                    egui::Sense::hover(),
                                                );
                                                ui.painter().rect_filled(rect, 2.0, track);
                                                let filled = egui::Rect::from_min_size(
                                                    rect.min,
                                                    egui::vec2(rect.width() * frac, rect.height()),
                                                );
                                                ui.painter().rect_filled(filled, 2.0, fill);
                                                ui.label(format!(
                                                    "{} free of {} ({:.0}% used)",
                                                    human_size(item_size),
                                                    human_size(item_total),
                                                    frac * 100.0
                                                ));
                                            });
                                        } else {
                                            ui.label(size_str);
                                        }
                                    });

                                    row.col(|ui| {
                                        if is_selected || is_multi_selected {
                                            ui.painter().rect_filled(ui.max_rect(), 0.0, self.theme.sel_bg);
                                        }
                                        ui.label(time_str);
                                    });
                                });
                            }
                        });

                    let remaining_height = (ui.clip_rect().bottom() - ui.cursor().top()).max(0.0);
                    if remaining_height > 0.0 {
                        let remaining_rect = egui::Rect::from_min_size(
                            ui.cursor().min,
                            egui::vec2(ui.available_width(), remaining_height),
                        );
                        let response =
                            ui.allocate_rect(remaining_rect, egui::Sense::click_and_drag());
                        response.context_menu(|ui| {
                            if ui.button("New Folder").clicked() {
                                pending_action = UiAction::NewFolder;
                                ui.close_menu();
                            }
                            if ui.button("New File").clicked() {
                                pending_action = UiAction::NewFile;
                                ui.close_menu();
                            }
                            if ui.button("Paste").clicked() {
                                pending_action = UiAction::Paste;
                                ui.close_menu();
                            }
                            if ui.button("Refresh").clicked() {
                                pending_action = UiAction::Refresh;
                                ui.close_menu();
                            }
                        });
                        if response.clicked() {
                            self.selected_item = None;
                            self.selected_items.clear();
                        }
                    }
                });

            if let Some(lr) = lasso_rect {
                stroke_rect(ui.painter(), lr, egui::Stroke::new(1.0_f32, self.theme.accent));
            }
            if ui.input(|i| i.pointer.primary_released()) {
                self.lasso_start = None;
            }
        });

        // ---- Action dispatch (unchanged) ----
        match pending_action {
            UiAction::None => {}
            UiAction::Copy => self.copy_or_cut("copy"),
            UiAction::Cut => self.copy_or_cut("cut"),
            UiAction::Paste => self.paste(),
            UiAction::Rename(name) => {
                self.active_dialog = Some(("Rename".to_string(), name));
            }
            UiAction::Delete => self.delete_selected(false),
            UiAction::TogglePin(path) => {
                let p = path.to_string_lossy().to_string();
                self.toggle_pin(&p);
            }
            UiAction::SidebarPin(path) => {
                let p = path.to_string_lossy().to_string();
                self.toggle_pin(&p);
            }
            UiAction::SidebarUnpin(path) => {
                let p = path.to_string_lossy().to_string();
                self.config.pins.retain(|x| x != &p);
                self.save_cfg();
            }
            UiAction::Open(path) | UiAction::OpenWith(path) => {
                if path.is_dir() {
                    navigate_to = Some(path.to_string_lossy().to_string());
                } else if let Err(e) = keyfm::apps::default_open(&path) {
                    self.status_msg = format!("Open failed: {}", e);
                }
            }
            UiAction::NewFolder => {
                self.active_dialog = Some(("NewFolder".to_string(), String::new()));
            }
            UiAction::NewFile => {
                self.active_dialog = Some(("NewFile".to_string(), String::new()));
            }
            UiAction::Refresh => self.refresh_keep_selection(),
        }

        if self.is_virtual_view() && self.active_dialog.is_some() {
            self.active_dialog = None;
            self.status_msg = "Not available in this view".to_string();
        }

        // ---- Dialogs (unchanged) ----
        if self.confirm_empty_bin {
            let (mut do_empty, mut close) = (false, false);
            egui::Window::new("Empty Recycle Bin")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(
                        "Permanently delete everything in the Recycle Bin? This cannot be undone.",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Empty").clicked() {
                            do_empty = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            if do_empty {
                self.status_msg = match keyfm::trash::win_empty_recycle_bin() {
                    Ok(()) => String::new(),
                    Err(e) => format!("Empty failed: {}", e),
                };
                self.refresh_dir();
                close = true;
            }
            if close {
                self.confirm_empty_bin = false;
            }
        }

        if let Some((action, mut input_val)) = self.active_dialog.take() {
            let fresh = !self.dialog_seen;
            self.dialog_seen = true;
            let mut confirm = false;
            let mut cancel = false;

            egui::Window::new(action.as_str())
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Enter name for {}:", action));
                    let resp = ui.text_edit_singleline(&mut input_val);
                    if fresh {
                        resp.request_focus();
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        confirm = true;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        cancel = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            confirm = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });

            let name = input_val.trim().to_string();

            if confirm && !name.is_empty() {
                let base = std::path::PathBuf::from(&self.cwd);
                let target = base.join(&name);

                let result = (|| -> Result<(), String> {
                    keyfm::helpers::validate_name(&name)?;
                    match action.as_str() {
                        "NewFolder" => {
                            if target.symlink_metadata().is_ok() {
                                return Err(format!("\"{}\" already exists", name));
                            }
                            std::fs::create_dir(&target)
                                .map_err(|e| format!("Create folder failed: {}", e))
                        }
                        "NewFile" => std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&target)
                            .map(|_| ())
                            .map_err(|e| {
                                if e.kind() == std::io::ErrorKind::AlreadyExists {
                                    format!("\"{}\" already exists", name)
                                } else {
                                    format!("Create file failed: {}", e)
                                }
                            }),
                        "Rename" => {
                            let idx = self.selected_item.ok_or("No item selected")?;
                            let old =
                                self.entries.get(idx).ok_or("No item selected")?.path.clone();
                            if old == target {
                                return Ok(());
                            }
                            let same_item = keyfm::helpers::norm(&old.to_string_lossy())
                                == keyfm::helpers::norm(&target.to_string_lossy());
                            if target.symlink_metadata().is_ok() && !same_item {
                                return Err(format!("\"{}\" already exists", name));
                            }
                            std::fs::rename(&old, &target)
                                .map_err(|e| format!("Rename failed: {}", e))
                        }
                        _ => Ok(()),
                    }
                })();

                match result {
                    Ok(()) => {
                        self.status_msg.clear();
                        self.refresh_dir();
                        if let Some(pos) = self.entries.iter().position(|it| it.path == target) {
                            self.selected_item = Some(pos);
                        }
                    }
                    Err(msg) => {
                        self.status_msg = msg;
                        self.active_dialog = Some((action, input_val));
                    }
                }
            } else if !cancel {
                self.active_dialog = Some((action, input_val));
            }

            if self.active_dialog.is_none() {
                self.dialog_seen = false;
            }
        }

        if nav_back {
            self.go_back();
        } else if nav_forward {
            self.go_forward();
        } else if let Some(raw) = navigate_to {
            match resolve_location(&raw, &self.cwd) {
                Some(loc) => {
                    self.go_to(loc);
                    self.selected_item = self.visible_indices().first().copied();
                }
                None => {
                    self.status_msg = format!("Not a directory: {}", raw);
                    self.path_input = self.cwd_display();
                }
            }
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let args: Vec<String> = env::args().collect();
    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            println!("keyfm {} - Keyboard-driven desktop file manager", keyfm::VERSION);
            println!("Usage: keyfm [START_DIR]");
            return Ok(());
        }
        if arg == "-V" || arg == "--version" {
            println!("keyfm {}", keyfm::VERSION);
            return Ok(());
        }
    }

    let start = args.get(1).cloned();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "keyfm",
        options,
        Box::new(|_cc| Ok(Box::new(KeyFmApp::new(start)))),
    )
}
