use eframe::egui;
use crate::app::KeyFmApp;
use crate::app::types::UiAction;
use crate::constants::{THIS_PC, RECYCLE};
use crate::helpers::same_loc;

impl KeyFmApp {
    pub(crate) fn handle_keyboard(
        &mut self,
        ctx: &egui::Context,
        navigate_to: &mut Option<String>,
        pending_action: &mut UiAction,
        nav_back: &mut bool,
        nav_forward: &mut bool,
    ) {
        if !ctx.wants_keyboard_input() && !self.modal_open() {
            if ctx.input(|i| i.key_pressed(egui::Key::Tab) && !i.modifiers.any()) {
                ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
                self.sidebar_focus = !self.sidebar_focus;
                if self.sidebar_focus {
                    let locs = self.sidebar_locations();
                    if let Some(pos) = locs.iter().position(|l| same_loc(l, &self.cwd)) {
                        self.sidebar_idx = pos;
                    } else { self.sidebar_idx = 0; }
                }
            }
        }

        if !ctx.wants_keyboard_input() && !self.modal_open() && self.sidebar_focus {
            let locs = self.sidebar_locations();
            let count = locs.len();
            if count > 0 {
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    if self.sidebar_idx + 1 < count { self.sidebar_idx += 1; }
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    self.sidebar_idx = self.sidebar_idx.saturating_sub(1);
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Home)) { self.sidebar_idx = 0; }
                if ctx.input(|i| i.key_pressed(egui::Key::End)) { self.sidebar_idx = count - 1; }
                
                if ctx.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::ArrowRight)) {
                    if let Some(loc) = locs.get(self.sidebar_idx) {
                        let p = std::path::PathBuf::from(loc);
                        if p.is_file() {
                            *pending_action = UiAction::Open(p);
                        } else {
                            *navigate_to = Some(loc.clone());
                        }
                        self.sidebar_focus = false;
                    }
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape) || (i.key_pressed(egui::Key::Tab) && !i.modifiers.any())) {
                if ctx.input(|i| i.key_pressed(egui::Key::Tab)) {
                    ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
                }
                self.sidebar_focus = false;
            }
        }

        if !ctx.wants_keyboard_input() && !self.modal_open() && !self.sidebar_focus {
            // File Ops
            if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.rendered_entries().get(idx) {
                        self.active_dialog = Some(("Rename".to_string(), item.name.clone()));
                    }
                }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::N)) {
                self.active_dialog = Some(("NewFolder".to_string(), String::new()));
            } else if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::N)) {
                self.active_dialog = Some(("NewFile".to_string(), String::new()));
            }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Delete)) {
                self.confirm_empty_bin = true;
            }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::C)) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.rendered_entries().get(idx) {
                        ctx.copy_text(item.path.to_string_lossy().to_string());
                        self.status_msg = "Path copied to clipboard".to_string();
                    }
                }
            }
            // View & System
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::H)) {
                self.config.show_hidden = !self.config.show_hidden;
                self.save_cfg();
                self.refresh_keep_selection();
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Num1)) {
                self.entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Num2)) {
                self.entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| b.size.cmp(&a.size)));
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Num3)) {
                self.entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(||
                    b.mtime.partial_cmp(&a.mtime).unwrap_or(std::cmp::Ordering::Equal)));
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::R)) { self.refresh_keep_selection(); }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Q)) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            // Quick access
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::P)) {
                if !self.is_virtual_view() {
                    let p = self.cwd.clone();
                    if !self.config.pins.contains(&p) {
                        self.config.pins.push(p);
                        self.save_cfg();
                        self.status_msg = "Pinned to Quick Access".to_string();
                    } else {
                        self.config.pins.retain(|x| x != &p);
                        self.save_cfg();
                        self.status_msg = "Unpinned from Quick Access".to_string();
                    }
                }
            }
            // Go to tab 1-9
            for i in 1..=9 {
                let key = match i {
                    1 => egui::Key::Num1, 2 => egui::Key::Num2, 3 => egui::Key::Num3,
                    4 => egui::Key::Num4, 5 => egui::Key::Num5, 6 => egui::Key::Num6,
                    7 => egui::Key::Num7, 8 => egui::Key::Num8, _ => egui::Key::Num9,
                };
                if ctx.input(|inp| inp.modifiers.alt && inp.key_pressed(key)) {
                    if self.tabs.len() >= i { self.load_tab(i - 1); }
                }
            }
            // Next / Prev tab
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::PageDown)) {
                if self.tabs.len() > 1 { self.load_tab((self.cur_tab + 1) % self.tabs.len()); }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::PageUp)) {
                if self.tabs.len() > 1 {
                    let prev = if self.cur_tab == 0 { self.tabs.len() - 1 } else { self.cur_tab - 1 };
                    self.load_tab(prev);
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Home) && !self.sidebar_focus) {
                let vis = self.visible_indices();
                if !vis.is_empty() { self.selected_item = Some(vis[0]); self.selected_items.clear(); }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::End) && !self.sidebar_focus) {
                let vis = self.visible_indices();
                if !vis.is_empty() { self.selected_item = Some(*vis.last().unwrap()); self.selected_items.clear(); }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::PageUp) && !self.sidebar_focus) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self.selected_item.and_then(|s| vis.iter().position(|&v| v == s)).unwrap_or(0);
                    self.selected_item = Some(vis[pos.saturating_sub(15)]);
                    self.selected_items.clear();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::PageDown) && !self.sidebar_focus) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self.selected_item.and_then(|s| vis.iter().position(|&v| v == s)).unwrap_or(0);
                    self.selected_item = Some(vis[(pos + 15).min(vis.len() - 1)]);
                    self.selected_items.clear();
                }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::M)) {
                *navigate_to = Some(THIS_PC.to_string());
            }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::B)) {
                *navigate_to = Some(RECYCLE.to_string());
            }
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::Home)) {
                if let Some(ud) = directories::UserDirs::new() {
                    *navigate_to = Some(ud.home_dir().to_string_lossy().to_string());
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Space) && !self.sidebar_focus) {
                if let Some(idx) = self.selected_item {
                    if self.selected_items.contains(&idx) { self.selected_items.retain(|&x| x != idx); }
                    else { self.selected_items.push(idx); }
                }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::A)) {
                self.selected_items = self.visible_indices();
            }
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::ArrowDown) && !self.sidebar_focus) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self.selected_item.and_then(|s| vis.iter().position(|&v| v == s)).unwrap_or(0);
                    let np = (pos + 1).min(vis.len() - 1);
                    self.selected_item = Some(vis[np]);
                    if !self.selected_items.contains(&vis[np]) { self.selected_items.push(vis[np]); }
                }
            }
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::ArrowUp) && !self.sidebar_focus) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self.selected_item.and_then(|s| vis.iter().position(|&v| v == s)).unwrap_or(0);
                    let np = pos.saturating_sub(1);
                    self.selected_item = Some(vis[np]);
                    if !self.selected_items.contains(&vis[np]) { self.selected_items.push(vis[np]); }
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown) && !self.sidebar_focus) {
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
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp) && !self.sidebar_focus) {
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
            // Left: jump to drive root
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft) && !i.modifiers.any() && !self.sidebar_focus) {
                if let Some(root) = self.drive_root() { *navigate_to = Some(root); }
            }
            // Shift+Left: cycle through Quick access
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::ArrowLeft) && !self.sidebar_focus) {
                if let Some(loc) = self.cycle_quick_access() { *navigate_to = Some(loc); }
            }
            // Right: enter folder / open file
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight) && !i.modifiers.any() && !self.sidebar_focus) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.rendered_entries().get(idx) {
                        if item.is_dir { *navigate_to = Some(item.path.to_string_lossy().to_string()); }
                        else { *pending_action = UiAction::Open(item.path.clone()); }
                    }
                }
            }
            // Enter
            if ctx.input(|i| i.key_pressed(egui::Key::Enter) && !self.sidebar_focus) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.rendered_entries().get(idx) {
                        if item.is_dir { *navigate_to = Some(item.path.to_string_lossy().to_string()); }
                        else { *pending_action = UiAction::Open(item.path.clone()); }
                    }
                }
            }
            // Ctrl+Shift+O: Open With
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::O)) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.rendered_entries().get(idx) {
                        if !item.is_dir { *pending_action = UiAction::OpenWith(item.path.clone()); }
                    }
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Backspace) && !i.modifiers.shift) {
                let has_history = self.tabs.get(self.cur_tab).map_or(false, |t| !t.back.is_empty());
                if has_history { *nav_back = true; } else { *navigate_to = self.parent_location(); }
            }
            if ctx.input(|i| (i.key_pressed(egui::Key::Backspace) && i.modifiers.shift)
                || (i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp))) {
                *navigate_to = self.parent_location();
            }
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) { *nav_back = true; }
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) { *nav_forward = true; }
            if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
                if self.search_active() { self.last_search_query.clear(); }
                else { self.refresh_keep_selection(); }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::C)) { self.copy_or_cut("copy"); }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::X)) { self.copy_or_cut("cut"); }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::V)) { self.paste(); }
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) { self.delete_selected(false); }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::D)) {
                self.config.dark = !self.config.dark;
                self.theme = if self.config.dark { crate::theme::DARK } else { crate::theme::LIGHT };
                self.save_cfg();
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::T)) { self.new_tab(); }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Tab)) {
                if self.tabs.len() > 1 {
                    let prev = if self.cur_tab == 0 { self.tabs.len() - 1 } else { self.cur_tab - 1 };
                    self.load_tab(prev);
                }
            } else if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Tab)) {
                if self.tabs.len() > 1 { self.load_tab((self.cur_tab + 1) % self.tabs.len()); }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::W)) {
                if self.tabs.len() > 1 { self.close_tab(); }
            }
        }
    }
}