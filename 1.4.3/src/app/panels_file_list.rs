use eframe::egui;
use crate::app::KeyFmApp;
use crate::app::types::UiAction;
use crate::app::ui_utils::{draw_icon, sel_fill, stroke_rect};
use crate::constants::{THIS_PC, RECYCLE, title_for, pc_title_for, bin_title_for};
use crate::helpers::{human_size, fmt_time};
use crate::icons::IconKind;

impl KeyFmApp {
    pub(crate) fn draw_file_list(
        &mut self,
        ctx: &egui::Context,
        navigate_to: &mut Option<String>,
        pending_action: &mut UiAction,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            let pointer_pos = ui.input(|i| i.pointer.interact_pos());
            let primary_down = ui.input(|i| i.pointer.primary_down());
            if ui.input(|i| i.pointer.primary_pressed()) {
                let on_panel = pointer_pos
                    .map(|p| ui.max_rect().contains(p) && ctx.layer_id_at(p) == Some(ui.layer_id()))
                    .unwrap_or(false);
                if on_panel { self.lasso_start = pointer_pos; self.selected_items.clear(); }
                else { self.lasso_start = None; }
            }
            let lasso_rect: Option<egui::Rect> = if primary_down {
                match (self.lasso_start, pointer_pos) {
                    (Some(start), Some(cur)) if start.distance(cur) > 4.0 =>
                        Some(egui::Rect::from_two_pos(start, cur)),
                    _ => None,
                }
            } else { None };
            if lasso_rect.is_some() { self.selected_items.clear(); self.selected_item = None; }

            egui::ScrollArea::vertical().auto_shrink([false, false]).drag_to_scroll(false).show(ui, |ui| {
                let filter = self.search_query.to_lowercase();
                let is_pc = self.cwd == THIS_PC;
                let in_bin = self.cwd == RECYCLE;
                let ppp = ctx.pixels_per_point();
                let row_h = if is_pc { 36.0 } else { 24.0 };
                let size_title = if is_pc { pc_title_for("size").unwrap_or("") }
                    else { title_for("size").unwrap_or("Size") };
                let mod_title = if is_pc { pc_title_for("modified").unwrap_or("") }
                    else if in_bin { bin_title_for("modified").unwrap_or("Date deleted") }
                    else { title_for("modified").unwrap_or("Modified") };
                let searching = self.search_active();
                
                // --- FIX: Temporarily take ownership of the lists so `self` isn't immutably borrowed ---
                let local_entries = if searching { Vec::new() } else { std::mem::take(&mut self.entries) };
                let local_search_results = if searching { std::mem::take(&mut self.search_results) } else { Vec::new() };
                let shown: &[crate::models::Item] = if searching { &local_search_results } else { &local_entries };

                let show_where = searching || in_bin;
                let size_w = self.config.size_col_width.clamp(60.0, 600.0);
                let mod_w  = self.config.mod_col_width.clamp(60.0, 400.0);
                let loc_w  = 240.0_f32;
                let row_right = ui.max_rect().right();
                let mut table = egui_extras::TableBuilder::new(ui)
                    .vscroll(false)
                    .resizable(true)
                    .column(egui_extras::Column::remainder().clip(true).at_least(120.0));
                if show_where {
                    table = table.column(
                        egui_extras::Column::initial(loc_w).at_least(80.0).clip(true),
                    );
                }
                table = table
                    .column(egui_extras::Column::initial(size_w).at_least(60.0).at_most(600.0))
                    .column(egui_extras::Column::initial(mod_w).at_least(60.0).at_most(400.0))
                    .header(20.0, |mut header| {
                        header.col(|ui| { ui.strong("Name"); });
                        if show_where {
                            header.col(|ui| {
                                let t = if in_bin {
                                    bin_title_for("where").unwrap_or("Deleted from")
                                } else {
                                    title_for("where").unwrap_or("Location")
                                };
                                ui.strong(t);
                            });
                        }
                        header.col(|ui| {
                            ui.strong(size_title);
                        header.col(|ui| {
                            ui.strong(mod_title);
                            let r = ui.max_rect();
                            let handle = egui::Rect::from_min_max(
                                egui::pos2(r.left(),       r.top()),
                                egui::pos2(r.left() + 8.0, r.bottom()),
                            );
                            let resp = ui.interact(
                                handle,
                                egui::Id::new("keyfm_resize_mod_col"),
                                egui::Sense::drag(),
                            );
                            if resp.hovered() || resp.dragged() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                            }
                            if resp.dragged() { mod_drag -= ui.input(|i| i.pointer.delta().x); }
                            if resp.drag_stopped() { drag_stopped = true; }
                        });
                    })
                    .body(|mut body| {
                        for (idx, item) in shown.iter().enumerate() {
                            if !filter.is_empty() && !item.name.to_lowercase().contains(&filter) { continue; }
                            let name_str = item.name.clone();
                            let size_str = if is_pc { human_size(item.size) }
                                else if item.is_dir { String::new() }
                                else { human_size(item.size) };
                            let time_str = fmt_time(item.mtime);
                            let is_dir = item.is_dir;
                            let item_path = item.path.clone();
                            let item_name = item.name.clone();
                            let item_orig = item.orig.clone();
                            let item_total = item.total;
                            let item_size = item.size;
                            let icon_name = item_orig.as_ref()
                                .and_then(|o| o.file_name())
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| item_name.clone());
                            let icon_kind = if is_pc { IconKind::Drive(&item_path) }
                                else if is_dir { IconKind::Folder }
                                else { IconKind::File { name: &icon_name, path: &item_path } };
                            let pin_str = item.path.to_string_lossy().to_string();
                            let is_pinned = self.config.pins.iter().any(|p| p == &pin_str);
                            let virt_view = self.is_virtual_view();
                            let can_pin = !in_bin && self.cwd != THIS_PC;

                            body.row(row_h, |mut row| {
                                row.col(|ui| {
                                    ui.spacing_mut().interact_size.y = row_h;
                                    let cell_rect = ui.max_rect();
                                    let row_rect = egui::Rect::from_min_max(
                                        cell_rect.min, egui::pos2(row_right, cell_rect.max.y));
                                    let cell_clip = ui.clip_rect();
                                    ui.set_clip_rect(egui::Rect::from_min_max(
                                        cell_clip.min, egui::pos2(row_right, cell_clip.max.y)));
                                    let response = ui.interact(row_rect,
                                        ui.id().with(("row_sel", idx)), egui::Sense::click());
                                    if let Some(lr) = lasso_rect {
                                        if lr.intersects(response.rect) {
                                            if !self.selected_items.contains(&idx) {
                                                self.selected_items.push(idx);
                                            }
                                            if self.selected_item.is_none() { self.selected_item = Some(idx); }
                                        }
                                    }
                                    ui.spacing_mut().interact_size.y = row_h;
                                    if self.selected_item == Some(idx) || self.selected_items.contains(&idx) {
                                        ui.painter().rect_filled(response.rect, 0.0, sel_fill(&self.theme, ui));
                                    } else if response.hovered() {
                                        ui.painter().rect_filled(response.rect, 0.0, self.theme.hover);
                                    }
                                    if response.clicked() { self.selected_item = Some(idx); }
                                    if response.secondary_clicked() {
                                        if !self.selected_items.contains(&idx) { self.selected_items.clear(); }
                                        self.selected_item = Some(idx);
                                    }
                                    if response.double_clicked() {
                                        if is_dir { *navigate_to = Some(item_path.to_string_lossy().to_string()); }
                                        else { *pending_action = UiAction::Open(item_path.clone()); }
                                    }
                                    ui.set_clip_rect(cell_clip);
                                    let visible = ui.is_rect_visible(ui.max_rect());
                                    ui.horizontal(|ui| {
                                        let tex = if visible { self.icons.get(ctx, icon_kind) } else { None };
                                        draw_icon(ui, ppp, tex.as_ref(), is_dir);
                                        ui.label(name_str);
                                        if let Some(o) = &item_orig {
                                            if !in_bin {
                                                ui.weak(format!("  (from {})", o.display()));
                                            }
                                        }
                                    });
                                    response.context_menu(|ui| {
                                        if in_bin {
                                            if ui.button("View full path").clicked() {
                                                if let Some(orig) = &item_orig {
                                                    *pending_action =
                                                        UiAction::ShowPathInStatus(orig.clone());
                                                }
                                                ui.close_menu();
                                            }
                                            ui.separator();
                                        }
                                        if ui.button("Open").clicked() {
                                            *pending_action = UiAction::Open(item_path.clone()); ui.close_menu();
                                        }
                                        if !is_dir && ui.button("Open With…").clicked() {
                                            *pending_action = UiAction::OpenWith(item_path.clone()); ui.close_menu();
                                        }
                                        if !virt_view {
                                            ui.separator();
                                            if ui.button("Copy").clicked() { *pending_action = UiAction::Copy; ui.close_menu(); }
                                            if ui.button("Cut").clicked() { *pending_action = UiAction::Cut; ui.close_menu(); }
                                            if ui.button("Paste").clicked() { *pending_action = UiAction::Paste; ui.close_menu(); }
                                            ui.separator();
                                            if ui.button("Rename").clicked() {
                                                *pending_action = UiAction::Rename(item_name.clone()); ui.close_menu();
                                            }
                                            if ui.button("Delete").clicked() { *pending_action = UiAction::Delete; ui.close_menu(); }
                                        }
                                        if can_pin {
                                            ui.separator();
                                            let label = if is_pinned { "Unpin from Quick Access" } else { "Pin to Quick Access" };
                                            if ui.button(label).clicked() {
                                                *pending_action = UiAction::TogglePin(item_path.clone()); ui.close_menu();
                                            }
                                        }
                                    });
                                });

                                if show_where {
                                    row.col(|ui| {
                                        if self.selected_item == Some(idx) || self.selected_items.contains(&idx) {
                                            ui.painter().rect_filled(ui.max_rect(), 0.0, sel_fill(&self.theme, ui));
                                        }
                                        let loc = if in_bin {
                                            item_orig.as_ref()
                                                .and_then(|o| o.parent())
                                                .map(|p| p.to_string_lossy().to_string())
                                                .unwrap_or_default()
                                        } else {
                                            item_path.parent()
                                                .map(|p| p.to_string_lossy().to_string())
                                                .unwrap_or_default()
                                        };
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(loc).color(self.theme.dim),
                                            )
                                            .truncate(),
                                        );
                                    });
                                }

                                row.col(|ui| {
                                    ui.spacing_mut().interact_size.y = row_h;
                                    if self.selected_item == Some(idx) || self.selected_items.contains(&idx) {
                                        ui.painter().rect_filled(ui.max_rect(), 0.0, sel_fill(&self.theme, ui));
                                    }
                                    if is_pc && item_total > 0 {
                                        let used = item_total.saturating_sub(item_size);
                                        let frac = (used as f32 / item_total as f32).clamp(0.0, 1.0);
                                        let fill = if frac > 0.9 { self.theme.danger } else { self.theme.accent };
                                        let track = self.theme.border;
                                        ui.vertical(|ui| {
                                            let w = (ui.available_width() - 12.0).max(60.0);
                                            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 8.0), egui::Sense::hover());
                                            ui.painter().rect_filled(rect, 2.0, track);
                                            let filled = egui::Rect::from_min_size(
                                                rect.min, egui::vec2(rect.width() * frac, rect.height()));
                                            ui.painter().rect_filled(filled, 2.0, fill);
                                            ui.label(format!("{} free of {} ({:.0}% used)",
                                                human_size(item_size), human_size(item_total), frac * 100.0));
                                        });
                                    } else {
                                        ui.horizontal(|ui| { ui.label(size_str); });
                                    }
                                });
                                row.col(|ui| {
                                    let r = ui.max_rect();
                                    if self.selected_item == Some(idx) || self.selected_items.contains(&idx) {
                                        ui.painter().rect_filled(r, 0.0, sel_fill(&self.theme, ui));
                                    }
                                    ui.painter().text(
                                        egui::pos2(r.left() + 4.0, r.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        &time_str,
                                        egui::TextStyle::Body.resolve(ui.style()),
                                        self.theme.dim);
                                });
                            });
                        }
                    });
                    let remaining_height = (ui.clip_rect().bottom() - ui.cursor().top()).max(0.0);
                    if remaining_height > 0.0 {
                    let remaining_rect = egui::Rect::from_min_size(
                        ui.cursor().min, egui::vec2(ui.available_width(), remaining_height));
                    let response = ui.allocate_rect(remaining_rect, egui::Sense::click_and_drag());
                    let virt = self.is_virtual_view();
                    response.context_menu(|ui| {
                        if ui.add_enabled(!virt, egui::Button::new("New Folder")).clicked() {
                            *pending_action = UiAction::NewFolder; ui.close_menu();
                        }
                        if ui.add_enabled(!virt, egui::Button::new("New File")).clicked() {
                            *pending_action = UiAction::NewFile; ui.close_menu();
                        }
                        if ui.add_enabled(!virt, egui::Button::new("Paste")).clicked() {
                            *pending_action = UiAction::Paste; ui.close_menu();
                        }
                        if ui.button("Refresh").clicked() {
                            *pending_action = UiAction::Refresh; ui.close_menu();
                        }
                    });
                    if response.clicked() {
                        self.selected_item = None;
                        self.selected_items.clear();
                        self.sidebar_focus = false;
                    }
                }

                // --- FIX: Put the lists back into self ---
                if searching {
                    self.search_results = local_search_results;
                } else {
                    self.entries = local_entries;
                }
            });

            if let Some(lr) = lasso_rect {
                stroke_rect(ui.painter(), lr, egui::Stroke::new(1.0_f32, self.theme.dim));
            }
            if ui.input(|i| i.pointer.primary_released()) { self.lasso_start = None; }
        });
    }
}