use eframe::egui;
use std::env;
use keyfm::config::{load_config, Config};
use keyfm::theme::{LIGHT, DARK, Theme};
use keyfm::helpers::get_start_dir;

use keyfm::models::Item;
use keyfm::drives;
use keyfm::helpers::{human_size, fmt_time, get_start_dir};
use std::path::Path;

struct KeyFmApp {
    config: Config,
    theme: Theme,
    cwd: String,
    entries: Vec<Item>,
    selected_item: Option<usize>,
    filter: String,
    path_input: String,
}

impl KeyFmApp {
    fn new(start: Option<String>) -> Self {
        let config = load_config();
        let theme = if config.dark { DARK } else { LIGHT };
        let start_dir = get_start_dir(start);
        let cwd = start_dir.to_string_lossy().to_string();
        let path_input = cwd.clone();
        let mut app = Self {
            config,
            theme,
            cwd,
            entries: Vec::new(),
            selected_item: None,
            filter: String::new(),
            path_input,
        };
        app.refresh_dir();
        app
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
        visuals.widgets.inactive.bg_fill = self.theme.surface;
        visuals.widgets.inactive.fg_stroke.color = self.theme.fg;
        visuals.widgets.hovered.bg_fill = self.theme.selection;
        visuals.widgets.hovered.fg_stroke.color = self.theme.fg;
        visuals.widgets.active.bg_fill = self.theme.selection;
        visuals.widgets.active.fg_stroke.color = self.theme.fg;
        visuals.selection.bg_fill = self.theme.selection;
        visuals.selection.stroke.color = self.theme.fg;
        ctx.set_visuals(visuals);

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Back").clicked() {
                    if let Some(parent) = Path::new(&self.cwd).parent() {
                        self.cwd = parent.to_string_lossy().to_string();
                        self.refresh_dir();
                    }
                }
                if ui.button("Forward").clicked() {
                    // Forward navigation placeholder
                }
                if ui.button("Up").clicked() {
                    if let Some(parent) = Path::new(&self.cwd).parent() {
                        self.cwd = parent.to_string_lossy().to_string();
                        self.refresh_dir();
                    }
                }
                ui.label("Path:");
                let resp = ui.text_editing_singleline(&mut self.path_input);
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.cwd = self.path_input.clone();
                    self.refresh_dir();
                }
                ui.label("Filter:");
                ui.text_editing_singleline(&mut self.filter);
            });
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{} items", self.entries.len()));
            });
        });

        egui::SidePanel::left("sidebar").show(ctx, |ui| {
            ui.heading("Quick Access");
            for (label, path) in drives::known_folders() {
                if ui.button(label).clicked() {
                    self.cwd = path.to_string_lossy().to_string();
                    self.refresh_dir();
                }
            }
            ui.separator();
            ui.heading("This PC");
            for (label, path) in drives::scan_drives() {
                if ui.button(label).clicked() {
                    self.cwd = path.to_string_lossy().to_string();
                    self.refresh_dir();
                }
            }
        });

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
                                            self.cwd = item.path.to_string_lossy().to_string();
                                            self.refresh_dir();
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
