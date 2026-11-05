use eframe::egui;
use crate::app::KeyFmApp;
use crate::app::ui_utils::{location_label, TOPBAR_ROW_H, PANEL_PAD_X, PANEL_PAD_TOP};

impl KeyFmApp {
    pub(crate) fn draw_tab_bar(&mut self, ctx: &egui::Context) {
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
                        let title = tab.cwd.as_ref()
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
                            egui::vec2(tab_width, tab_height), egui::Sense::click());
                        let rounding = egui::Rounding { nw: 8.0, ne: 8.0, sw: 0.0, se: 0.0 };
                        let bg = if is_active { theme.tab_sel } else { theme.tab };
                        ui.painter().rect_filled(rect, rounding, bg);
                        if response.hovered() && !is_active {
                            ui.painter().rect_filled(rect, rounding, theme.hover);
                        }
                        let text_pos = egui::pos2(rect.left() + padding,
                            rect.center().y - galley.size().y / 2.0);
                        ui.painter().galley(text_pos, galley, text_color);
                        if tab_count > 1 {
                            let close_rect = egui::Rect::from_center_size(
                                egui::pos2(rect.right() - close_w / 2.0 - 3.0, rect.center().y),
                                egui::vec2(14.0, 14.0));
                            let close_hovered = response.hovered() && ui.input(|i| i.pointer.hover_pos())
                                .map_or(false, |p| close_rect.contains(p));
                            let close_color = if close_hovered { theme.danger } else { theme.dim };
                            let stroke = egui::Stroke::new(1.5_f32, close_color);
                            let c = close_rect.center();
                            let h = 4.0;
                            ui.painter().line_segment(
                                [egui::pos2(c.x - h, c.y - h), egui::pos2(c.x + h, c.y + h)], stroke);
                            ui.painter().line_segment(
                                [egui::pos2(c.x + h, c.y - h), egui::pos2(c.x - h, c.y + h)], stroke);
                            if response.clicked() {
                                let pos = response.interact_pointer_pos().unwrap_or(rect.center());
                                if close_rect.contains(pos) { close_tab_idx = Some(i); }
                                else if !is_active { switch_to_tab = Some(i); }
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
                        egui::vec2(plus_w, tab_height), egui::Sense::click());
                    if plus_response.hovered() {
                        ui.painter().rect_filled(plus_rect, 6.0, theme.hover);
                    }
                    let plus_color = if plus_response.hovered() { theme.fg } else { theme.dim };
                    let c = plus_rect.center();
                    let h = 5.0;
                    let stroke = egui::Stroke::new(1.5_f32, plus_color);
                    ui.painter().line_segment(
                        [egui::pos2(c.x - h, c.y), egui::pos2(c.x + h, c.y)], stroke);
                    ui.painter().line_segment(
                        [egui::pos2(c.x, c.y - h), egui::pos2(c.x, c.y + h)], stroke);
                    if plus_response.clicked() { new_tab_clicked = true; }
                    if let Some(i) = close_tab_idx { self.close_tab_at(i); }
                    if let Some(tab_idx) = switch_to_tab {
                        if tab_idx != self.cur_tab { self.load_tab(tab_idx); }
                    }
                    if new_tab_clicked { self.new_tab(); }
                });
                ui.add_space(4.0);
            });
    }

    pub(crate) fn draw_toolbar(
        &mut self,
        ctx: &egui::Context,
        navigate_to: &mut Option<String>,
        nav_back: &mut bool,
        nav_forward: &mut bool,
    ) {
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
                    if ui.add_enabled(can_back, egui::Button::new("Back")).clicked() { *nav_back = true; }
                    if ui.add_enabled(can_fwd, egui::Button::new("Forward")).clicked() { *nav_forward = true; }
                    if ui.add_enabled(can_up, egui::Button::new("Up")).clicked() {
                        *navigate_to = self.parent_location();
                    }
                    ui.label("Path:");
                    let resp = ui.text_edit_singleline(&mut self.path_input);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        *navigate_to = Some(self.path_input.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(PANEL_PAD_X);
                        let search_resp = ui.add_sized([200.0, 20.0],
                            egui::TextEdit::singleline(&mut self.search_query).hint_text("Search"));
                        if search_resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            self.search_query.clear();
                        }
                    });
                });
                let row_bottom = row_top + TOPBAR_ROW_H;
                let sep = egui::Stroke::new(1.0_f32, self.theme.border);
                ui.painter().line_segment(
                    [egui::pos2(panel_rect.left(), row_bottom),
                     egui::pos2(panel_rect.right(), row_bottom)], sep);
            });
    }

    pub(crate) fn draw_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.search_active() && self.search_running {
                    ui.label(format!("Searching... ({} found)", self.search_results.len()));
                } else if self.search_active() && self.search_truncated {
                    ui.label(format!("{}+ found for \"{}\" (capped)",
                                     self.search_results.len(), self.search_query));
                } else if self.search_active() {
                    ui.label(format!("{} found for \"{}\"",
                                     self.search_results.len(), self.search_query));
                } else {
                    let total = self.entries.len();
                    let shown_n = if self.search_query.is_empty() { total }
                        else { self.visible_indices().len() };
                    if shown_n == total { ui.label(format!("{} items", total)); }
                    else { ui.label(format!("{} of {} items", shown_n, total)); }
                }
                if !self.status_msg.is_empty() {
                    ui.separator();
                    ui.label(&self.status_msg);
                }
            });
        });
    }
}