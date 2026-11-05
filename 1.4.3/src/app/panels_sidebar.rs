use eframe::egui;
use crate::app::KeyFmApp;
use crate::app::types::UiAction;
use crate::app::ui_utils::{draw_icon, TOPBAR_ROW_H, PANEL_PAD_X, PANEL_PAD_TOP};
use crate::constants::{THIS_PC, QUICK_ACCESS, VIRTUAL};
use crate::helpers::same_loc;
use crate::icons::IconKind;
use crate::theme::{LIGHT, DARK};

impl KeyFmApp {
    pub(crate) fn draw_sidebar(
        &mut self,
        ctx: &egui::Context,
        navigate_to: &mut Option<String>,
        pending_action: &mut UiAction,
    ) {
        let sidebar_inner = egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(self.config.sidebar_width)
            .min_width(120.0)
            .max_width(600.0)
            .frame(egui::Frame::default().fill(self.theme.bg))
            .show(ctx, |ui| {
                let panel_rect = ui.max_rect();
                ui.add_space(PANEL_PAD_TOP);
                let menu_row_top = ui.cursor().top();
                ui.horizontal(|ui| {
                    ui.set_min_height(TOPBAR_ROW_H);
                    ui.set_max_height(TOPBAR_ROW_H);
                    ui.add_space(PANEL_PAD_X);
                    ui.menu_button("File", |ui| {
                        if ui.button("New Tab").clicked() { self.new_tab(); ui.close_menu(); }
                        if ui.button("Close Tab").clicked() { self.close_tab(); ui.close_menu(); }
                        if ui.button("New Folder").clicked() {
                            self.active_dialog = Some(("NewFolder".to_string(), "".to_string()));
                            ui.close_menu();
                        }
                        if ui.button("New File").clicked() {
                            self.active_dialog = Some(("NewFile".to_string(), "".to_string()));
                            ui.close_menu();
                        }
                        if ui.button("Empty Recycle Bin").clicked() {
                            self.confirm_empty_bin = true; ui.close_menu();
                        }
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button("Edit", |ui| {
                        if ui.button("Copy").clicked() { self.copy_or_cut("copy"); ui.close_menu(); }
                        if ui.button("Cut").clicked() { self.copy_or_cut("cut"); ui.close_menu(); }
                        if ui.button("Paste").clicked() { self.paste(); ui.close_menu(); }
                        if ui.button("Rename").clicked() {
                            if let Some(idx) = self.selected_item {
                                if let Some(item) = self.entries.get(idx) {
                                    self.active_dialog = Some(("Rename".to_string(), item.name.clone()));
                                }
                            }
                            ui.close_menu();
                        }
                        if ui.button("Delete (Recycle)").clicked() { self.delete_selected(false); ui.close_menu(); }
                        if ui.button("Delete Permanently").clicked() { self.delete_selected(true); ui.close_menu(); }
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
                        if ui.button("Keyboard Shortcuts (F1)").clicked() {
                            self.show_shortcuts = true; ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Refresh").clicked() {
                            self.refresh_keep_selection(); ui.close_menu();
                        }
                    });
                });
                let menu_row_bottom = menu_row_top + TOPBAR_ROW_H;
                let sep = egui::Stroke::new(1.0_f32, self.theme.border);
                ui.painter().line_segment(
                    [egui::pos2(panel_rect.left(), menu_row_bottom),
                     egui::pos2(panel_rect.right(), menu_row_bottom)], sep);
                ui.painter().line_segment(
                    [egui::pos2(panel_rect.right() - 0.5, panel_rect.top()),
                     egui::pos2(panel_rect.right() - 0.5, panel_rect.bottom())], sep);
                ui.add_space(10.0);

                let sb_locs = self.sidebar_locations();
                let qa_pos = sb_locs.iter().position(|l| same_loc(l, QUICK_ACCESS)).unwrap_or(0);
                let is_qa_selected = self.cwd == QUICK_ACCESS
                    || (self.sidebar_focus && self.sidebar_idx == qa_pos);
                if ui.selectable_label(is_qa_selected,
                        egui::RichText::new("Quick access").strong()).clicked()
                {
                    *navigate_to = Some(QUICK_ACCESS.to_string());
                    self.sidebar_focus = true;
                    self.sidebar_idx = qa_pos;
                }
                ui.add_space(4.0);

                if !self.config.pins.is_empty() {
                    for pin in &self.config.pins {
                        let label = std::path::Path::new(pin).file_name()
                            .map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| pin.clone());
                        let pin_path = std::path::PathBuf::from(pin);
                        let is_selected = same_loc(&self.cwd, pin)
                            || (self.sidebar_focus && self.sidebar_idx == sb_locs.iter()
                                .position(|l| same_loc(l, pin)).unwrap_or(0));
                        let resp = ui.selectable_label(is_selected, label);
                        if resp.clicked() {
                            if pin_path.is_dir() {
                                *navigate_to = Some(pin.clone());
                            } else if pin_path.is_file() {
                                *pending_action = UiAction::Open(pin_path.clone());
                            } else {
                                self.status_msg = format!("Pin no longer available: {}", pin);
                            }
                            self.sidebar_focus = true;
                            self.sidebar_idx = sb_locs.iter().position(|l| same_loc(l, pin)).unwrap_or(0);
                        }
                        let pin_path_for_menu = pin_path.clone();
                        resp.context_menu(|ui| {
                            if ui.button("Open").clicked() {
                                let target = pin_path_for_menu.clone();
                                if target.is_dir() {
                                    *navigate_to = Some(target.to_string_lossy().to_string());
                                } else if target.is_file() {
                                    *pending_action = UiAction::Open(target);
                                }
                                ui.close_menu();
                            }
                            if ui.button("Unpin").clicked() {
                                *pending_action = UiAction::SidebarUnpin(pin_path_for_menu.clone());
                                ui.close_menu();
                            }
                        });
                    }
                    ui.add_space(4.0);
                }

                let known_cache = self.known_cache.clone();
                for (label, path) in &known_cache {
                    let path_str = path.to_string_lossy().to_string();
                    let is_selected = same_loc(&self.cwd, &path_str)
                        || (self.sidebar_focus && self.sidebar_idx == sb_locs.iter()
                            .position(|l| same_loc(l, &path_str)).unwrap_or(0));
                    let resp = ui.selectable_label(is_selected, *label);
                    if resp.clicked() {
                        *navigate_to = Some(path_str.clone());
                        self.sidebar_focus = true;
                        self.sidebar_idx = sb_locs.iter().position(|l| same_loc(l, &path_str)).unwrap_or(0);
                    }
                    let path_str_c = path_str.clone();
                    resp.context_menu(|ui| {
                        if ui.button("Open").clicked() {
                            *navigate_to = Some(path_str_c.clone());
                            ui.close_menu();
                        }
                        if !VIRTUAL.contains(&path_str_c.as_str()) {
                            if ui.button("Pin").clicked() {
                                *pending_action = UiAction::SidebarPin(std::path::PathBuf::from(&path_str_c));
                                ui.close_menu();
                            }
                            if ui.button("Unpin").clicked() {
                                *pending_action = UiAction::SidebarUnpin(std::path::PathBuf::from(&path_str_c));
                                ui.close_menu();
                            }
                        }
                    });
                }

                ui.add_space(8.0);
                let is_pc_selected = self.cwd == THIS_PC
                    || (self.sidebar_focus && self.sidebar_idx == sb_locs.iter()
                        .position(|l| same_loc(l, THIS_PC)).unwrap_or(0));
                if ui.selectable_label(is_pc_selected, egui::RichText::new("This PC").strong()).clicked() {
                    *navigate_to = Some(THIS_PC.to_string());
                    self.sidebar_focus = true;
                    self.sidebar_idx = sb_locs.iter().position(|l| same_loc(l, THIS_PC)).unwrap_or(0);
                }

                let drives_info_cache = self.drives_info_cache.clone();
                let ppp = ctx.pixels_per_point();
                for (label, path, free, total) in &drives_info_cache {
                    let path_str = path.to_string_lossy().to_string();
                    let is_selected = same_loc(&self.cwd, &path_str)
                        || (self.sidebar_focus && self.sidebar_idx == sb_locs.iter()
                            .position(|l| same_loc(l, &path_str)).unwrap_or(0));
                    let usage = if *total > 0 { ((*total - *free) as f64 / *total as f64) * 100.0 } else { 0.0 };
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        let tex = self.icons.get(ctx, IconKind::Drive(path));
                        draw_icon(ui, ppp, tex.as_ref(), false);
                        let text = format!("{} {:.1}%", label, usage);
                        if ui.selectable_label(is_selected, text).clicked() {
                            *navigate_to = Some(path_str.clone());
                            self.sidebar_focus = true;
                            self.sidebar_idx = sb_locs.iter().position(|l| same_loc(l, &path_str)).unwrap_or(0);
                        }
                    });
                }
            });

        let sidebar_width = sidebar_inner.response.rect.width();
        if (sidebar_width - self.config.sidebar_width).abs() > 1.0 {
            self.config.sidebar_width = sidebar_width;
            self.save_cfg();
        }
    }
}