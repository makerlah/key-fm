use eframe::egui;
use crate::app::KeyFmApp;
use crate::app::types::UiAction;
use crate::app::search::{SearchHit, run_walk, SEARCH_DEBOUNCE_MS, SEARCH_RESULT_CAP};
use crate::drives;
use std::path::PathBuf;

impl eframe::App for KeyFmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut navigate_to: Option<String> = None;
        let mut pending_action = UiAction::None;
        let mut nav_back = false;
        let mut nav_forward = false;
        self.icons.begin_frame();

        self.update_search(ctx);
        self.update_theme(ctx);

        self.draw_tab_bar(ctx);
        self.draw_sidebar(ctx, &mut navigate_to, &mut pending_action);
        self.draw_toolbar(ctx, &mut navigate_to, &mut nav_back, &mut nav_forward);
        self.draw_shortcuts_dialog(ctx);
        self.handle_keyboard(ctx, &mut navigate_to, &mut pending_action, &mut nav_back, &mut nav_forward);
        self.draw_status_bar(ctx);
        self.draw_file_list(ctx, &mut navigate_to, &mut pending_action);
        self.dispatch_pending_actions(pending_action, &mut navigate_to);
        self.draw_empty_bin_dialog(ctx);
        self.draw_open_with_dialog(ctx);
        self.draw_rename_dialog(ctx);
        self.dispatch_navigation(&navigate_to, nav_back, nav_forward);
    }
}

impl KeyFmApp {
    fn update_search(&mut self, ctx: &egui::Context) {
        if self.search_query != self.last_search_query {
            self.selected_item = None;
            self.selected_items.clear();
            if self.search_query.chars().count() <= 1 {
                self.cancel_search();
            } else {
                self.search_debounce = Some(std::time::Instant::now());
            }
            self.last_search_query = self.search_query.clone();
        }

        if let Some(t) = self.search_debounce {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            if t.elapsed().as_millis() >= SEARCH_DEBOUNCE_MS as u128 {
                self.search_debounce = None;
                self.cancel_search();
                let (tx, rx) = std::sync::mpsc::channel::<SearchHit>();
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let needle = self.search_query.to_lowercase();
                let roots: Vec<PathBuf> = if self.cwd == crate::constants::THIS_PC {
                    drives::scan_drives().into_iter().map(|(_, p)| p).collect()
                } else if self.cwd == crate::constants::RECYCLE {
                    Vec::new()
                } else {
                    vec![PathBuf::from(&self.cwd)]
                };
                let mut handles = Vec::new();
                for root in roots {
                    let tx = tx.clone();
                    let cancel = cancel.clone();
                    let needle = needle.clone();
                    let show_hidden = self.config.show_hidden;
                    handles.push(std::thread::spawn(move || {
                        run_walk(root, needle, show_hidden, cancel, tx);
                    }));
                }
                drop(tx);
                self.search_rx = Some(rx);
                self.search_cancel = Some(cancel);
                self.search_threads = Some(handles);
                self.search_running = true;
                self.search_truncated = false;
                self.search_results.clear();
            }
        }

        if self.search_running || self.search_rx.is_some() { ctx.request_repaint(); }

        if let Some(rx) = self.search_rx.take() {
            loop {
                match rx.try_recv() {
                    Ok(hit) => {
                        if self.search_results.len() < SEARCH_RESULT_CAP {
                            self.search_results.push(hit.into_item());
                        } else {
                            self.search_truncated = true;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        self.search_rx = Some(rx);
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.search_running = false;
                        self.search_results.sort_by(|a, b| {
                            b.is_dir.cmp(&a.is_dir)
                                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                        });
                        if let Some(ts) = self.search_threads.take() {
                            for t in ts { let _ = t.join(); }
                        }
                        break;
                    }
                }
            }
        }
    }

    fn update_theme(&mut self, ctx: &egui::Context) {
        let mut visuals = if self.config.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
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

        let mut style = (*ctx.style()).clone();
        style.visuals.window_rounding = 8.0.into();
        style.visuals.menu_rounding = 8.0.into();
        style.visuals.widgets.noninteractive.rounding = 4.0.into();
        style.visuals.widgets.inactive.rounding = 4.0.into();
        style.visuals.widgets.hovered.rounding = 4.0.into();
        style.visuals.widgets.active.rounding = 4.0.into();
        style.visuals.window_shadow = egui::epaint::Shadow {
            offset: egui::vec2(0.0, 8.0), blur: 24.0, spread: 0.0,
            color: egui::Color32::from_black_alpha(60),
        };
        style.visuals.popup_shadow = egui::epaint::Shadow {
            offset: egui::vec2(0.0, 4.0), blur: 12.0, spread: 0.0,
            color: egui::Color32::from_black_alpha(40),
        };
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 32.0;
        style.spacing.window_margin = egui::Margin::same(16.0);
        ctx.set_style(style);
    }

    fn dispatch_pending_actions(&mut self, pending_action: UiAction, navigate_to: &mut Option<String>) {
        match pending_action {
            UiAction::None => {}
            UiAction::Copy => self.copy_or_cut("copy"),
            UiAction::Cut => self.copy_or_cut("cut"),
            UiAction::Paste => self.paste(),
            UiAction::Rename(name) => { self.active_dialog = Some(("Rename".to_string(), name)); }
            UiAction::Delete => self.delete_selected(false),
            UiAction::TogglePin(path) => { let p = path.to_string_lossy().to_string(); self.toggle_pin(&p); }
            UiAction::SidebarPin(path) => { let p = path.to_string_lossy().to_string(); self.toggle_pin(&p); }
            UiAction::SidebarUnpin(path) => {
                let p = path.to_string_lossy().to_string();
                self.config.pins.retain(|x| x != &p);
                self.save_cfg();
            }
            UiAction::Open(path) => {
                if path.is_dir() { *navigate_to = Some(path.to_string_lossy().to_string()); }
                else {
                    match crate::apps::default_open(&path) {
                        Ok(()) => {}
                        Err(_) => self.open_with = Some(crate::app::OpenWithState::new(path)),
                    }
                }
            }
            UiAction::OpenWith(path) => {
                if !path.is_dir() { self.open_with = Some(crate::app::OpenWithState::new(path)); }
                else { *navigate_to = Some(path.to_string_lossy().to_string()); }
            }
            UiAction::NewFolder => { self.active_dialog = Some(("NewFolder".to_string(), String::new())); }
            UiAction::NewFile => { self.active_dialog = Some(("NewFile".to_string(), String::new())); }
            UiAction::Refresh => {
                if self.search_active() { self.last_search_query.clear(); }
                else { self.refresh_keep_selection(); }
            }
        }

        if self.is_virtual_view() && self.active_dialog.is_some() {
            self.active_dialog = None;
            self.status_msg = "Not available in this view".to_string();
        }
    }

    fn dispatch_navigation(&mut self, navigate_to: &Option<String>, nav_back: bool, nav_forward: bool) {
        if nav_back { self.go_back(); }
        else if nav_forward { self.go_forward(); }
        else if let Some(raw) = navigate_to {
            match crate::app::ui_utils::resolve_location(raw, &self.cwd) {
                Some(loc) => {
                    self.go_to(loc);
                    if !self.sidebar_focus {
                        self.selected_item = self.visible_indices().first().copied();
                    }
                }
                None => {
                    self.status_msg = format!("Not a directory: {}", raw);
                    self.path_input = self.cwd_display();
                }
            }
        }
    }
}