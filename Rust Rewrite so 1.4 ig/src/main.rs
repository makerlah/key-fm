use eframe::egui;
use std::env;
use keyfm::config::{load_config, Config};
use keyfm::theme::{LIGHT, DARK, Theme};
use keyfm::helpers::get_start_dir;

use keyfm::models::{Item, Tab};
use keyfm::drives;
use keyfm::helpers::{human_size, fmt_time};
use std::path::Path;

/// Version-independent rectangle border (avoids egui's changing
/// `rect_stroke` signature across versions).
fn stroke_rect(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    painter.line_segment([rect.left_top(), rect.right_top()], stroke);
    painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
    painter.line_segment([rect.right_bottom(), rect.left_bottom()], stroke);
    painter.line_segment([rect.left_bottom(), rect.left_top()], stroke);
}

/// Actions queued from inside table row / context-menu closures.
///
/// They are collected in `update()` and executed *after* the table is
/// rendered, so we never have to mutate `self` while `self.entries` is
/// still borrowed by the row iterator.
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
    EmptySpace,
    EmptyBin,
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
    clipboard: Vec<std::path::PathBuf>,
    clip_mode: String,
    active_dialog: Option<(String, String)>,
    lasso_start: Option<egui::Pos2>,
    selected_items: Vec<usize>,
    status_msg: String,
}

impl KeyFmApp {
    fn new(start: Option<String>) -> Self {
        let config = load_config();
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
        };
        app.refresh_dir();
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
        if self.tabs.len() > 1 {
            self.tabs.remove(self.cur_tab);
            if self.cur_tab >= self.tabs.len() {
                self.cur_tab = self.tabs.len() - 1;
            }
            let idx = self.cur_tab;
            self.load_tab(idx);
        }
    }

    fn copy_or_cut(&mut self, mode: &str) {
        if let Some(idx) = self.selected_item {
            if let Some(item) = self.entries.get(idx) {
                self.clipboard = vec![item.path.clone()];
                self.clip_mode = mode.to_string();
            }
        }
    }

    fn paste(&mut self) {
        if self.clipboard.is_empty() { return; }
        let dest_dir = std::path::PathBuf::from(&self.cwd);
        
        for src in &self.clipboard {
            if src.is_dir() {
                eprintln!("paste: skipping directory (not supported yet): {}", src.display());
                continue;
            }
            if let Some(file_name) = src.file_name() {
                let dest = dest_dir.join(file_name);
                if dest == *src {
                    continue;
                }
                if self.clip_mode == "copy" {
                    let _ = std::fs::copy(src, &dest); 
                } else {
                    let _ = std::fs::rename(src, &dest);
                }
            }
        }
        
        if self.clip_mode == "cut" {
            self.clipboard.clear();
        }
        self.refresh_dir();
    }

    fn delete_selected(&mut self, permanent: bool) {
        let mut error_msg: Option<String> = None;
        if let Some(idx) = self.selected_item {
            if let Some(item) = self.entries.get(idx) {
                let path = item.path.clone();
                if permanent {
                    let res = if path.is_dir() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                    if let Err(e) = res {
                        error_msg = Some(format!("Delete failed: {}", e));
                    }
                } else if let Err(e) = keyfm::trash::move_to_trash(&path) {
                    error_msg = Some(format!("Trash failed: {}", e));
                }
                self.selected_item = None;
            }
        }
        if let Some(msg) = error_msg {
            self.status_msg = msg;
        }
        self.refresh_dir();
    }

    fn toggle_pin(&mut self, path: &str) {
        if let Some(pos) = self.config.pins.iter().position(|p| p == path) {
            self.config.pins.remove(pos);
        } else {
            self.config.pins.push(path.to_string());
        }
        let _ = keyfm::config::save_config(&self.config);
    }

    fn refresh_dir(&mut self) {
        self.entries.clear();
        let path = Path::new(&self.cwd);
        if let Ok(read_dir) = std::fs::read_dir(path) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                if !self.config.show_hidden && name.starts_with('.') {
                    continue;
                }
                let metadata = entry.metadata().ok();
                let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = metadata.as_ref().map(|m| if is_dir { 0 } else { m.len() }).unwrap_or(0);
                let mtime = metadata.as_ref().and_then(|m| m.modified().ok()).and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64())
                });
                let hidden = name.starts_with('.');

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
        self.path_input = self.cwd.clone();
        self.selected_item = None;
        self.selected_items.clear();
    }
}

impl eframe::App for KeyFmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut navigate_to: Option<String> = None;
        let mut pending_action = UiAction::None;

        // 1. Apply Theme
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

        // Top Menu Bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
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
                        self.active_dialog = Some(("NewFolder".to_string(), "".to_string()));
                        ui.close_menu();
                    }
                    if ui.button("New File").clicked() {
                        self.active_dialog = Some(("NewFile".to_string(), "".to_string()));
                        ui.close_menu();
                    }
                    if ui.button("Empty Recycle Bin").clicked() {
                        let _ = keyfm::trash::win_empty_recycle_bin();
                        self.refresh_dir();
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
                                self.active_dialog = Some(("Rename".to_string(), item.name.clone()));
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
                ui.menu_button("View", |_ui| {});
                ui.menu_button("Go", |_ui| {});
            });
        });

        // Tab Bar
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let mut switch_to_tab: Option<usize> = None;
                for (i, tab) in self.tabs.iter().enumerate() {
                    let title = tab.cwd.as_ref()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                        .unwrap_or_else(|| "Tab".to_string());
                    
                    // 2. Change what happens on click
                    if ui.selectable_label(self.cur_tab == i, title).clicked() {
                        switch_to_tab = Some(i);
                    }
                }

                if let Some(tab_idx) = switch_to_tab {
                    self.load_tab(tab_idx);
                }

                if ui.button("+").clicked() {
                    self.new_tab();
                }
                if self.tabs.len() > 1 && ui.button("x").clicked() {
                    self.close_tab();
                }
            });
        });

        // SidePanel (Sidebar)
                egui::SidePanel::left("sidebar").show(ctx, |ui| {
            ui.heading("Quick Access");
            if !self.config.pins.is_empty() {
                for pin in &self.config.pins {
                    let label = std::path::Path::new(pin)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| pin.clone());
                    let pin_path = std::path::PathBuf::from(pin);
                    let resp = ui.button(format!("📌 {}", label));
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
                ui.separator();
            }
            for (label, path) in drives::known_folders() {
                let path_str = path.to_string_lossy().to_string();
                let resp = ui.button(label);
                if resp.clicked() {
                    navigate_to = Some(path_str.clone());
                }
                resp.context_menu(|ui| {
                    if ui.button("Open").clicked() {
                        navigate_to = Some(path_str.clone());
                        ui.close_menu();
                    }
                    if ui.button("Pin").clicked() {
                        pending_action = UiAction::SidebarPin(std::path::PathBuf::from(&path_str));
                        ui.close_menu();
                    }
                    if ui.button("Unpin").clicked() {
                        pending_action =
                            UiAction::SidebarUnpin(std::path::PathBuf::from(&path_str));
                        ui.close_menu();
                    }
                });
            }
            ui.separator(); 
            ui.heading("This PC");
            // scan_drives returns a tuple (String, PathBuf), so we destructure it here
            for (name, path) in drives::scan_drives() {
                if ui.button(&name).clicked() {
                    navigate_to = Some(path.to_string_lossy().to_string());
                }
            }
        });

        // 2. Keyboard Input Handling
        if !ctx.wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                let max_idx = self.entries.len().saturating_sub(1);
                self.selected_item = Some(match self.selected_item {
                    Some(idx) => (idx + 1).min(max_idx),
                    None => 0,
                });
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                self.selected_item = Some(match self.selected_item {
                    Some(idx) => idx.saturating_sub(1),
                    None => 0,
                });
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
            if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) {
                if let Some(parent) = std::path::Path::new(&self.cwd).parent() {
                    navigate_to = Some(parent.to_string_lossy().to_string());
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
                self.refresh_dir();
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
            if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.entries.get(idx) {
                        self.active_dialog = Some(("Rename".to_string(), item.name.clone()));
                    }
                }
            }
        }

        // 3. Top Toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Back").clicked() {
                    if let Some(parent) = std::path::Path::new(&self.cwd).parent() {
                        navigate_to = Some(parent.to_string_lossy().to_string());
                    }
                }
                if ui.button("Forward").clicked() {
                    // Forward navigation placeholder
                }
                if ui.button("Up").clicked() {
                    if let Some(parent) = std::path::Path::new(&self.cwd).parent() {
                        navigate_to = Some(parent.to_string_lossy().to_string());
                    }
                }
                ui.label("Path:");
                let resp = ui.text_edit_singleline(&mut self.path_input);
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    navigate_to = Some(self.path_input.clone());
                }
                ui.label("Filter:");
                ui.text_edit_singleline(&mut self.filter);
            });
        });

        // 4. Bottom Status Bar
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{} items", self.entries.len()));
                if !self.status_msg.is_empty() {
                    ui.separator();
                    ui.label(&self.status_msg);
                }
            });
        });

        // 5. Central Panel (File List)
        egui::CentralPanel::default().show(ctx, |ui| {
            // ---- lasso state ----
            let pointer_pos = ui.input(|i| i.pointer.interact_pos());
            let primary_down = ui.input(|i| i.pointer.primary_down());
            if ui.input(|i| i.pointer.primary_pressed()) {
                self.lasso_start = pointer_pos;
                self.selected_items.clear();
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

            // ---- empty space (behind the rows) ----
            let empty_response = ui.interact(
                ui.available_rect_before_wrap(),
                ui.id().with("empty_space"),
                egui::Sense::click(),
            );
            if empty_response.clicked() {
                pending_action = UiAction::EmptySpace;
            }
            empty_response.context_menu(|ui| {
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

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let filter = self.filter.to_lowercase();
                egui_extras::TableBuilder::new(ui)
                    .vscroll(false)
                    .column(egui_extras::Column::remainder())
                    .column(egui_extras::Column::exact(100.0))
                    .column(egui_extras::Column::exact(150.0))
                    .header(20.0, |mut header| {
                        header.col(|ui| { ui.strong("Name"); });
                        header.col(|ui| { ui.strong("Size"); });
                        header.col(|ui| { ui.strong("Modified"); });
                    })
                    .body(|mut body| {
                        for (idx, item) in self.entries.iter().enumerate() {
                            if !filter.is_empty() && !item.name.to_lowercase().contains(&filter) {
                                continue;
                            }
                            let name_str = if item.is_dir {
                                format!("{}/", item.name)
                            } else {
                                item.name.clone()
                            };
                            let size_str = if item.is_dir {
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
                            let pin_str = item.path.to_string_lossy().to_string();
                            let is_pinned = self.config.pins.iter().any(|p| p == &pin_str);

                            body.row(18.0, |mut row| {
                                // ---- column 1: interaction + text ----
                                row.col(|ui| {
                                    let response = ui.interact(
                                        ui.max_rect(),
                                        ui.id().with(("row_sel", idx)),
                                        egui::Sense::click(),
                                    );

                                    // Lasso: select every row the drag rectangle touches.
                                    if let Some(lr) = lasso_rect {
                                        if lr.intersects(response.rect) {
                                            if !self.selected_items.contains(&idx) {
                                                self.selected_items.push(idx);
                                            }
                                            self.selected_item = Some(idx);
                                        }
                                    }

                                    // Paint the selection highlight *before* the text.
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
                                            egui::Stroke::new(1.0, self.theme.accent),
                                        );
                                    }

                                    if response.clicked() || response.secondary_clicked() {
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

                                    ui.label(name_str);

                                    response.context_menu(|ui| {
                                        if ui.button("Open").clicked() {
                                            pending_action = UiAction::Open(item_path.clone());
                                            ui.close_menu();
                                        }
                                        if ui.button("Open With (default app)").clicked() {
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
                                            pending_action =
                                                UiAction::Rename(item_name.clone());
                                            ui.close_menu();
                                        }
                                        if ui.button("Delete").clicked() {
                                            pending_action = UiAction::Delete;
                                            ui.close_menu();
                                        }
                                        if is_dir {
                                            ui.separator();
                                            let label = if is_pinned {
                                                "Unpin from Quick Access"
                                            } else {
                                                "Pin to Quick Access"
                                            };
                                            if ui.button(label).clicked() {
                                                pending_action = UiAction::TogglePin(
                                                    item_path.clone(),
                                                );
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                });

                                // ---- column 2: size ----
                                row.col(|ui| {
                                    if is_selected || is_multi_selected {
                                        ui.painter().rect_filled(
                                            ui.max_rect(),
                                            0.0,
                                            self.theme.sel_bg,
                                        );
                                    }
                                    ui.label(size_str);
                                });

                                // ---- column 3: modified time ----
                                row.col(|ui| {
                                    if is_selected || is_multi_selected {
                                        ui.painter().rect_filled(
                                            ui.max_rect(),
                                            0.0,
                                            self.theme.sel_bg,
                                        );
                                    }
                                    ui.label(time_str);
                                });
                            });
                        }
                    });

                // ---- blank space below the rows: context menu + click-to-deselect ----
                // Fill whatever is left of the visible viewport under the table.
                let remaining_height = (ui.clip_rect().bottom() - ui.cursor().top()).max(0.0);
                if remaining_height > 0.0 {
                    let remaining_rect = egui::Rect::from_min_size(
                        ui.cursor().min,
                        egui::vec2(ui.available_width(), remaining_height),
                    );
                    let response = ui.allocate_rect(remaining_rect, egui::Sense::click());
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
                    // Also clear selection if they click empty space
                    if response.clicked() {
                        self.selected_item = None;
                    }
                }
            });

            // Draw the lasso rectangle on top of the rows.
            if let Some(lr) = lasso_rect {
                stroke_rect(
                    ui.painter(),
                    lr,
                    egui::Stroke::new(1.0, self.theme.accent),
                );
            }
            if ui.input(|i| i.pointer.primary_released()) {
                self.lasso_start = None;
            }
        });

        // Execute anything the row / context-menu closures queued up. Done
        // here so `self.entries` is no longer borrowed by the table loop.
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
                let _ = keyfm::config::save_config(&self.config);
            }
            UiAction::Open(path) | UiAction::OpenWith(path) => {
                if path.is_dir() {
                    navigate_to = Some(path.to_string_lossy().to_string());
                } else if let Err(e) = keyfm::apps::default_open(&path) {
                    self.status_msg = format!("Open failed: {}", e);
                }
            }
            UiAction::EmptySpace => {
                self.selected_item = None;
                self.selected_items.clear();
            }
            UiAction::EmptyBin => {
                let _ = keyfm::trash::win_empty_recycle_bin();
                self.refresh_dir();
            }
            UiAction::NewFolder => {
                self.active_dialog = Some(("NewFolder".to_string(), String::new()));
            }
            UiAction::NewFile => {
                self.active_dialog = Some(("NewFile".to_string(), String::new()));
            }
            UiAction::Refresh => self.refresh_dir(),
        }

        // Dialog rendering (take() so we never hold a borrow of self while
        // touching the filesystem or refreshing the directory listing).
        if let Some((action, mut input_val)) = self.active_dialog.take() {
            let mut confirm = false;
            let mut cancel = false;

            egui::Window::new(action.as_str())
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Enter name for {}:", action));
                    let resp = ui.text_edit_singleline(&mut input_val);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        confirm = true;
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
                let mut succeeded = false;

                match action.as_str() {
                    "NewFolder" => match std::fs::create_dir(base.join(&name)) {
                        Ok(()) => succeeded = true,
                        Err(e) => eprintln!("NewFolder failed: {}", e),
                    },
                    "NewFile" => match std::fs::File::create(base.join(&name)) {
                        Ok(_) => succeeded = true,
                        Err(e) => eprintln!("NewFile failed: {}", e),
                    },
                    "Rename" => {
                        if let Some(idx) = self.selected_item {
                            if let Some(item) = self.entries.get(idx) {
                                let old_path = item.path.clone();
                                let new_path = base.join(&name);
                                match std::fs::rename(&old_path, &new_path) {
                                    Ok(()) => succeeded = true,
                                    Err(e) => eprintln!(
                                        "Rename failed ({} -> {}): {}",
                                        old_path.display(),
                                        new_path.display(),
                                        e
                                    ),
                                }
                            } else {
                                eprintln!("Rename failed: no item selected");
                            }
                        } else {
                            eprintln!("Rename failed: no item selected");
                        }
                    }
                    _ => {}
                }

                self.refresh_dir();

                // Keep the just-created / renamed entry selected.
                if succeeded {
                    if let Some(pos) = self.entries.iter().position(|it| it.name == name) {
                        self.selected_item = Some(pos);
                    }
                }
                // Dialog is closed (we already took() it).
            } else if !cancel {
                // Keep it open and preserve whatever the user typed.
                self.active_dialog = Some((action, input_val));
            }
        }

        // 6. Apply Navigation (Must be at the very end!)
        if let Some(new_path) = navigate_to {
            self.cwd = new_path;
            self.refresh_dir();
            self.selected_item = Some(0);
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let args: Vec<String> = env::args().collect();
    for arg in &args {
        if arg == "-h" || arg == "--help" {
            println!("keyfm 1.3.0 - Keyboard-driven desktop file manager");
            println!("Usage: keyfm [START_DIR]");
            return Ok(());
        }
        if arg == "-V" || arg == "--version" {
            println!("keyfm 1.3.0");
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
