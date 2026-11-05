use eframe::egui;
use crate::app::KeyFmApp;
use crate::app::open_with::PickerOutcome;
use crate::app::open_with_picker::program_picker_ui;

impl KeyFmApp {
    pub(crate) fn draw_open_with_dialog(&mut self, ctx: &egui::Context) {
        if self.open_with.is_none() { return; }
        let mut launch_exe: Option<String> = None;
        let mut close_dialog = false;

        if let Some(state) = self.open_with.as_mut() {
            let target_name = state.target.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| state.target.display().to_string());
            let mut start_picker = false;

            egui::Window::new("Open With").collapsible(false).resizable(true)
                .default_width(420.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(format!("Choose an application to open \"{}\":", target_name));
                    ui.add_space(6.0);
                    let apps = crate::apps::list_apps();
                    let filter_resp = ui.add(egui::TextEdit::singleline(&mut state.filter)
                        .hint_text("Filter applications… (Enter opens the first match)")
                        .desired_width(f32::INFINITY));
                    if state.focus_filter { filter_resp.request_focus(); state.focus_filter = false; }
                    ui.add_space(4.0);

                    let needle = state.filter.trim().to_lowercase();
                    let matches: Vec<&(String, String)> = apps.iter()
                        .filter(|a| needle.is_empty() || a.0.to_lowercase().contains(&needle))
                        .collect();

                    if filter_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if let Some(first) = matches.first() {
                            launch_exe = Some(first.1.clone());
                            close_dialog = true;
                        }
                    }

                    if apps.is_empty() {
                        ui.weak("No installed applications found. Use \"Choose another program…\".");
                    } else if matches.is_empty() {
                        ui.weak("No applications match the filter.");
                    } else {
                        egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                            for app in &matches {
                                let btn = egui::Button::new(app.0.as_str())
                                    .min_size(egui::vec2(ui.available_width(), 22.0));
                                if ui.add(btn).on_hover_text(app.1.as_str()).clicked() {
                                    launch_exe = Some(app.1.clone());
                                    close_dialog = true;
                                }
                            }
                        });
                    }

                    ui.add_space(6.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Choose another program…").clicked() { start_picker = true; }
                        if ui.button("Cancel").clicked() { close_dialog = true; }
                    });
                });

            if start_picker { state.start_picker(); }

            if state.picker_open {
                let vid = egui::ViewportId::from_hash_of("keyfm_program_picker");
                let builder = egui::ViewportBuilder::default()
                    .with_title("Open with…")
                    .with_inner_size([860.0, 560.0])
                    .with_min_inner_size([560.0, 340.0]);
                let theme = self.theme;
                let icons = &mut self.icons;
                let known = &self.known_cache;
                let drives_c = &self.drives_info_cache;
                let mut outcome = PickerOutcome { choose: None, close: false };

                ctx.show_viewport_immediate(vid, builder, |vctx, class| {
                    icons.begin_frame();
                    let mut body = |ui: &mut egui::Ui| -> PickerOutcome {
                        program_picker_ui(ui, vctx, state, icons, &theme, known, drives_c, &target_name)
                    };
                    if class == egui::ViewportClass::Embedded {
                        egui::Window::new("Open with…").default_size([780.0, 500.0]).show(vctx, |ui| {
                            outcome = body(ui);
                        });
                    } else {
                        egui::CentralPanel::default().show(vctx, |ui| {
                            outcome = body(ui);
                        });
                    }
                    if vctx.input(|i| i.viewport().close_requested()) { outcome.close = true; }
                });

                if let Some(p) = outcome.choose {
                    launch_exe = Some(p.to_string_lossy().to_string());
                    close_dialog = true;
                } else if outcome.close {
                    state.picker_open = false;
                }
            }

            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                if state.picker_open { state.picker_open = false; }
                else { close_dialog = true; }
            }
        }

        if let Some(exe) = launch_exe {
            let target = self.open_with.as_ref().map(|s| s.target.clone());
            self.open_with = None;
            if let Some(target) = target {
                match crate::apps::launch_with(&exe, &target) {
                    Ok(()) => {
                        let app = std::path::Path::new(&exe).file_stem()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| exe.clone());
                        self.status_msg = format!("Opened with {}", app);
                    }
                    Err(e) => self.status_msg = format!("Could not launch {}: {}", exe, e),
                }
            }
        } else if close_dialog {
            self.open_with = None;
        }
    }
}