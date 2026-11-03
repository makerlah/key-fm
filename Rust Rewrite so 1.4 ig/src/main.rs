use eframe::egui;
use std::env;
use keyfm::config::{load_config, Config};
use keyfm::theme::{LIGHT, DARK, Theme};
use keyfm::helpers::get_start_dir;

use keyfm::models::{Item, Tab};
use keyfm::drives;
use keyfm::helpers::{human_size, fmt_time};
use std::path::Path;

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
            if let Some(file_name) = src.file_name() {
                let dest = dest_dir.join(file_name);
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
        if let Some(idx) = self.selected_item {
            if let Some(item) = self.entries.get(idx) {
                let path = item.path.clone();
                if path.is_dir() {
                    let _ = std::fs::remove_dir_all(&path);
                } else {
                    let _ = std::fs::remove_file(&path);
                }
                self.selected_item = None; 
            }
        }
        self.refresh_dir();
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
    }
}

impl eframe::App for KeyFmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut navigate_to: Option<String> = None;

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
            for (label, path) in drives::known_folders() {
                if ui.button(label).clicked() {
                    // Convert PathBuf to String
                    navigate_to = Some(path.to_string_lossy().to_string());
                }
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
            });
        });

        // 5. Central Panel (File List)
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let filter = self.filter.to_lowercase();
                egui_extras::TableBuilder::new(ui)
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
                            let name_str = if item.is_dir { format!("{}/", item.name) } else { item.name.clone() };
                            let size_str = if item.is_dir { "".to_string() } else { human_size(item.size) };
                            let time_str = fmt_time(item.mtime);

                            body.row(18.0, |mut row| {
                                row.col(|ui| {
                                    let resp = ui.selectable_label(self.selected_item == Some(idx), name_str);
                                    if resp.clicked() {
                                        self.selected_item = Some(idx);
                                    }
                                    if resp.double_clicked() {
                                        if item.is_dir {
                                            navigate_to = Some(item.path.to_string_lossy().to_string());
                                        }
                                    }
                                });
                                row.col(|ui| {
                                    ui.label(size_str);
                                });
                                row.col(|ui| {
                                    ui.label(time_str);
                                });
                            });
                        }
                    });
            });
        });

        // Dialog rendering
        if let Some((action, ref mut input_val)) = self.active_dialog {
            let mut close_dialog = false;
            let mut confirm = false;
            egui::Window::new(&action)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Enter name for {}:", action));
                    let resp = ui.text_edit_singleline(input_val);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        confirm = true;
                    }
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            confirm = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_dialog = true;
                        }
                    });
                });

            if confirm {
                let current_cwd = self.cwd.clone();
                let target_path = Path::new(&current_cwd);
                if action == "NewFolder" {
                    let _ = std::fs::create_dir(target_path.join(input_val));
                    self.refresh_dir();
                } else if action == "NewFile" {
                    let _ = std::fs::File::create(target_path.join(input_val));
                    self.refresh_dir();
                } else if action == "Rename" {
                    if let Some(idx) = self.selected_item {
                        if let Some(item) = self.entries.get(idx) {
                            let old_path = item.path.clone();
                            let new_path = target_path.join(input_val);
                            let _ = std::fs::rename(old_path, new_path);
                            self.refresh_dir();
                        }
                    }
                }
                close_dialog = true;
            }

            if close_dialog {
                self.active_dialog = None;
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
