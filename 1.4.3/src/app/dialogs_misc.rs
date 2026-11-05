use eframe::egui;
use crate::app::KeyFmApp;
use crate::helpers::validate_name;

impl KeyFmApp {
    pub(crate) fn draw_shortcuts_dialog(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_shortcuts = !self.show_shortcuts;
        }
        if !self.show_shortcuts { return; }
        let dim_color = self.theme.dim;
        let mut is_open = true;
        let mut close_clicked = false;

        struct Section {
            title: &'static str,
            rows: &'static [(&'static str, &'static str)],
        }
        let sections: &[Section] = &[
            Section { title: "Tabs", rows: &[
                ("New tab", "Ctrl+T"),
                ("Close tab", "Ctrl+W"),
                ("Next tab", "Ctrl+Tab / Ctrl+PgDn"),
                ("Previous tab", "Ctrl+Shift+Tab / Ctrl+PgUp"),
                ("Go to tab 1-9", "Alt+1 … Alt+9"),
            ]},
            Section { title: "Files", rows: &[
                ("Select all", "Ctrl+A"),
                ("Copy", "Ctrl+C"),
                ("Cut", "Ctrl+X"),
                ("Paste", "Ctrl+V"),
                ("Rename", "F2"),
                ("Delete to bin", "Delete"),
                ("Delete permanently", "Shift+Delete"),
                ("New file", "Ctrl+N"),
                ("New folder", "Ctrl+Shift+N"),
                ("Open with…", "Right-click → Open With"),
            ]},
            Section { title: "Navigation", rows: &[
                ("Open / enter folder", "Enter / Right"),
                ("Back in history", "Alt+Left"),
                ("Back / up", "Backspace"),
                ("Forward", "Alt+Right"),
                ("Up one folder", "Alt+Up / Shift+Backspace"),
                ("Go to drive root", "Left"),
                ("Next Quick access folder", "Shift+Left"),
                ("Home folder", "Alt+Home"),
                ("This PC", "Ctrl+Shift+M"),
                ("Recycle Bin", "Ctrl+Shift+B"),
                ("First item", "Home"),
                ("Last item", "End"),
                ("Page up", "PgUp"),
                ("Page down", "PgDn"),
            ]},
            Section { title: "Quick access", rows: &[
                ("Pin / unpin folder", "Ctrl+P"),
            ]},
            Section { title: "View", rows: &[
                ("Toggle hidden files", "Ctrl+H"),
                ("Dark / light mode", "Ctrl+Shift+D"),
                ("Sort by name", "Ctrl+1"),
                ("Sort by size", "Ctrl+2"),
                ("Sort by date", "Ctrl+3"),
                ("Refresh", "F5 / Ctrl+R"),
            ]},
            Section { title: "Fixed keys", rows: &[
                ("Up / Down", "Move selection"),
                ("Shift+Up / Shift+Down", "Extend selection"),
                ("Space", "Toggle selection"),
                ("Esc", "Clear / close"),
                ("Click + drag", "Lasso select"),
                ("F1", "Toggle this help"),
                ("Ctrl+Q", "Quit"),
            ]},
        ];

        egui::Window::new("Keyboard Shortcuts")
            .open(&mut is_open).collapsible(false).resizable(true)
            .default_width(1100.0).min_width(400.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let avail = ui.available_width();
                let num_cols = if avail >= 850.0 { 3 } else if avail >= 500.0 { 2 } else { 1 };
                let per_col: &[usize] = match num_cols { 3 => &[2, 2, 2], 2 => &[3, 3], _ => &[6] };
                egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(800.0).show(ui, |ui| {
                    ui.columns(num_cols, |cols| {
                        let mut idx = 0usize;
                        for (ci, &count) in per_col.iter().enumerate() {
                            let col_ui = &mut cols[ci];
                            for si in 0..count {
                                if idx >= sections.len() { break; }
                                let sec = &sections[idx];
                                if si > 0 { col_ui.add_space(12.0); }
                                egui::Grid::new(("sc_grid", ci, si)).num_columns(2)
                                    .spacing([20.0, 4.0]).striped(true).show(col_ui, |ui| {
                                        ui.heading(sec.title);
                                        ui.label("");
                                        ui.end_row();
                                        for (label, keys) in sec.rows {
                                            ui.label(*label);
                                            ui.label(egui::RichText::new(*keys).color(dim_color));
                                            ui.end_row();
                                        }
                                    });
                                idx += 1;
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() { close_clicked = true; }
                });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) || close_clicked || !is_open {
            self.show_shortcuts = false;
        }
    }

    pub(crate) fn draw_empty_bin_dialog(&mut self, ctx: &egui::Context) {
        if !self.confirm_empty_bin { return; }
        let (mut do_empty, mut close) = (false, false);
        egui::Window::new("Empty Recycle Bin").collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label("Permanently delete everything in the Recycle Bin? This cannot be undone.");
            ui.horizontal(|ui| {
                if ui.button("Empty").clicked() { do_empty = true; }
                if ui.button("Cancel").clicked() { close = true; }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) { close = true; }
        });
        if do_empty {
            self.status_msg = match crate::trash::win_empty_recycle_bin() {
                Ok(()) => String::new(),
                Err(e) => format!("Empty failed: {}", e),
            };
            self.refresh_dir();
            close = true;
        }
        if close { self.confirm_empty_bin = false; }
    }

    pub(crate) fn draw_rename_dialog(&mut self, ctx: &egui::Context) {
        let Some((action, mut input_val)) = self.active_dialog.take() else { return; };
        let fresh = !self.dialog_seen;
        self.dialog_seen = true;
        let mut confirm = false;
        let mut cancel = false;

        egui::Window::new(action.as_str()).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(format!("Enter name for {}:", action));
            let resp = ui.text_edit_singleline(&mut input_val);
            if fresh { resp.request_focus(); }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { confirm = true; }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) { cancel = true; }
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() { confirm = true; }
                if ui.button("Cancel").clicked() { cancel = true; }
            });
        });

        let name = input_val.trim().to_string();
        if confirm && !name.is_empty() {
            let base = std::path::PathBuf::from(&self.cwd);
            let target = base.join(&name);
            let result = (|| -> Result<(), String> {
                validate_name(&name)?;
                match action.as_str() {
                    "NewFolder" => {
                        if target.symlink_metadata().is_ok() {
                            return Err(format!("\"{}\" already exists", name));
                        }
                        std::fs::create_dir(&target).map_err(|e| format!("Create folder failed: {}", e))
                    }
                    "NewFile" => std::fs::OpenOptions::new().write(true).create_new(true).open(&target)
                        .map(|_| ())
                        .map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists {
                            format!("\"{}\" already exists", name)
                        } else { format!("Create file failed: {}", e) }),
                    "Rename" => {
                        let idx = self.selected_item.ok_or("No item selected")?;
                        let old = self.rendered_entries().get(idx).ok_or("No item selected")?.path.clone();
                        if old == target { return Ok(()); }
                        let same_item = crate::helpers::norm(&old.to_string_lossy())
                            == crate::helpers::norm(&target.to_string_lossy());
                        if target.symlink_metadata().is_ok() && !same_item {
                            return Err(format!("\"{}\" already exists", name));
                        }
                        std::fs::rename(&old, &target).map_err(|e| format!("Rename failed: {}", e))
                    }
                    _ => Ok(()),
                }
            })();
            match result {
                Ok(()) => {
                    self.status_msg.clear();
                    self.refresh_dir();
                    if let Some(pos) = self.rendered_entries().iter().position(|it| it.path == target) {
                        self.selected_item = Some(pos);
                    }
                }
                Err(msg) => { self.status_msg = msg; self.active_dialog = Some((action, input_val)); }
            }
        } else if !cancel {
            self.active_dialog = Some((action, input_val));
        }
        if self.active_dialog.is_none() { self.dialog_seen = false; }
    }
}