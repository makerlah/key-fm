use crate::app::open_with::{OpenWithState, PickerOutcome, PICKER_ROW_H, picker_shortcuts};
use crate::app::ui_utils::draw_icon;
use crate::helpers::{same_loc, human_size, fmt_time, expand_and_validate_in};
use crate::icons::{IconCache, IconKind};
use crate::theme::Theme;
use eframe::egui;
use std::path::{Path, PathBuf};

pub(crate) fn program_picker_ui(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    st: &mut OpenWithState,
    icons: &mut IconCache,
    theme: &Theme,
    known: &[(&'static str, PathBuf)],
    drives: &[(String, PathBuf, u64, u64)],
    target_name: &str,
) -> PickerOutcome {
    let mut out = PickerOutcome { choose: None, close: false };
    let ppp = ctx.pixels_per_point();

    let mut goto: Option<PathBuf> = None;
    let mut activate: Option<usize> = None;
    let mut go_back = false;
    let mut go_fwd = false;
    let mut go_up = false;
    let mut path_submit = false;
    let mut reload = false;

    let needle = st.picker_filter.trim().to_lowercase();
    let visible: Vec<usize> = st.picker_entries.iter().enumerate()
        .filter(|(_, e)| needle.is_empty() || e.name.to_lowercase().contains(&needle))
        .map(|(i, _)| i).collect();

    egui::TopBottomPanel::top("pk_top").show_inside(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!st.picker_hist.is_empty(), egui::Button::new("←"))
                .on_hover_text("Back (Alt+Left)").clicked() { go_back = true; }
            if ui.add_enabled(!st.picker_fwd.is_empty(), egui::Button::new("→"))
                .on_hover_text("Forward (Alt+Right)").clicked() { go_fwd = true; }
            if ui.button("↑").on_hover_text("Up (Backspace)").clicked() { go_up = true; }
            let w = (ui.available_width() - 200.0).max(120.0);
            let r = ui.add(egui::TextEdit::singleline(&mut st.picker_path_input).desired_width(w));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { path_submit = true; }
            let f = ui.add(egui::TextEdit::singleline(&mut st.picker_filter)
                .hint_text("Filter (Ctrl+F)").desired_width(f32::INFINITY));
            if st.picker_focus_filter { f.request_focus(); st.picker_focus_filter = false; }
        });
        ui.add_space(4.0);
    });

    let sel_file: Option<PathBuf> = st.picker_selected
        .and_then(|i| st.picker_entries.get(i)).filter(|e| !e.is_dir)
        .map(|e| e.path.clone());
    egui::TopBottomPanel::bottom("pk_bottom").show_inside(ui, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let shown = sel_file.as_ref().and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "select a program".to_string());
            ui.label(format!("Open \"{}\" with:", target_name));
            ui.label(egui::RichText::new(shown).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() { out.close = true; }
                if ui.add_enabled(sel_file.is_some(), egui::Button::new("Open")).clicked() {
                    out.choose = sel_file.clone();
                }
                if ui.checkbox(&mut st.picker_show_all, "All files").changed() { reload = true; }
            });
        });
        ui.add_space(6.0);
    });

    let shortcuts = picker_shortcuts(known);
    egui::SidePanel::left("pk_side")
        .resizable(true).default_width(190.0).width_range(140.0..=320.0)
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(6.0);
                ui.weak("Quick access");
                for (label, path) in &shortcuts {
                    if !path.is_dir() { continue; }
                    let cur = same_loc(&st.picker_cwd.to_string_lossy(), &path.to_string_lossy());
                    ui.horizontal(|ui| {
                        let tex = icons.get(ctx, IconKind::Folder);
                        draw_icon(ui, ppp, tex.as_ref(), true);
                        if ui.selectable_label(cur, label.as_str()).clicked() { goto = Some(path.clone()); }
                    });
                }
                ui.add_space(8.0);
                ui.weak("Drives");
                for (label, path, _, _) in drives {
                    let cur = same_loc(&st.picker_cwd.to_string_lossy(), &path.to_string_lossy());
                    ui.horizontal(|ui| {
                        let tex = icons.get(ctx, IconKind::Drive(path));
                        draw_icon(ui, ppp, tex.as_ref(), false);
                        if ui.selectable_label(cur, label.as_str()).clicked() { goto = Some(path.clone()); }
                    });
                }
            });
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let (hr, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::hover());
        let size_right = hr.right() - 14.0;
        let mod_left = size_right - 90.0 - 150.0;
        let hp = ui.painter();
        hp.text(egui::pos2(hr.left() + 30.0, hr.center().y), egui::Align2::LEFT_CENTER,
                "Name", font.clone(), theme.dim);
        hp.text(egui::pos2(mod_left, hr.center().y), egui::Align2::LEFT_CENTER,
                "Date modified", font.clone(), theme.dim);
        hp.text(egui::pos2(size_right, hr.center().y), egui::Align2::RIGHT_CENTER,
                "Size", font.clone(), theme.dim);
        hp.line_segment([hr.left_bottom(), hr.right_bottom()], egui::Stroke::new(1.0_f32, theme.border));

        if visible.is_empty() {
            ui.add_space(12.0);
            if st.picker_entries.is_empty() {
                ui.weak("No programs or folders here. Tick \"All files\" to see everything.");
            } else {
                ui.weak("Nothing matches the filter.");
            }
            return;
        }

        let mut sa = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(off) = st.picker_set_offset.take() { sa = sa.vertical_scroll_offset(off); }
        let res = sa.show_rows(ui, PICKER_ROW_H, visible.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in range {
                let i = visible[row];
                let (name, path, is_dir, size, mtime) = {
                    let e = &st.picker_entries[i];
                    (e.name.clone(), e.path.clone(), e.is_dir, e.size, e.mtime)
                };
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), PICKER_ROW_H), egui::Sense::click());
                let selected = st.picker_selected == Some(i);
                let fg = if selected { theme.sel_fg } else { ui.visuals().text_color() };
                let dim = if selected { theme.sel_fg } else { theme.dim };
                let kind = if is_dir { IconKind::Folder }
                    else { IconKind::File { name: &name, path: &path } };
                let tex = icons.get(ctx, kind);

                let painter = ui.painter();
                if selected { painter.rect_filled(rect, 3.0, theme.sel_bg); }
                else if resp.hovered() { painter.rect_filled(rect, 3.0, theme.hover); }

                let icon_size = tex.as_ref().map(|t| t.size_vec2() / ppp)
                    .unwrap_or_else(|| egui::vec2(16.0, 16.0));
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 16.0, rect.center().y), icon_size);
match tex.as_ref() {
    Some(t) => {
        painter.image(t.id(), icon_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE);
    }
    None if is_dir => {
        painter.rect_filled(
            icon_rect.shrink2(egui::vec2(1.0, 3.0)), 2.0,
            egui::Color32::from_rgb(0xf0, 0xc0, 0x4a));
    }
    None => {}
}
                let r_size = rect.right() - 14.0;
                let r_mod = r_size - 90.0 - 150.0;
                let name_clip = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 30.0, rect.top()),
                    egui::pos2(r_mod - 8.0, rect.bottom()));
                painter.with_clip_rect(name_clip).text(
                    egui::pos2(rect.left() + 30.0, rect.center().y),
                    egui::Align2::LEFT_CENTER, &name, font.clone(), fg);
                painter.text(egui::pos2(r_mod, rect.center().y), egui::Align2::LEFT_CENTER,
                             fmt_time(mtime), font.clone(), dim);
                if !is_dir {
                    painter.text(egui::pos2(r_size, rect.center().y), egui::Align2::RIGHT_CENTER,
                                 human_size(size), font.clone(), dim);
                }
                if resp.clicked() { st.picker_selected = Some(i); }
                if resp.double_clicked() { activate = Some(i); }
            }
        });

        if let Some(pos) = st.picker_scroll_to.take() {
            let top = res.state.offset.y;
            let h = res.inner_rect.height();
            let row_top = pos as f32 * PICKER_ROW_H;
            if row_top < top { st.picker_set_offset = Some(row_top); ctx.request_repaint(); }
            else if row_top + PICKER_ROW_H > top + h {
                st.picker_set_offset = Some(row_top + PICKER_ROW_H - h); ctx.request_repaint();
            }
        }
    });

    if !ctx.wants_keyboard_input() && !visible.is_empty() {
        let cur_pos = st.picker_selected.and_then(|s| visible.iter().position(|&v| v == s));
        let last = visible.len() - 1;
        let mut new_pos: Option<usize> = None;
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            new_pos = Some(cur_pos.map_or(0, |p| (p + 1).min(last)));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            new_pos = Some(cur_pos.map_or(0, |p| p.saturating_sub(1)));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Home)) { new_pos = Some(0); }
        if ctx.input(|i| i.key_pressed(egui::Key::End)) { new_pos = Some(last); }
        if let Some(np) = new_pos {
            st.picker_selected = Some(visible[np]);
            st.picker_scroll_to = Some(np);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(s) = st.picker_selected { activate = Some(s); }
        }
    }
    if !ctx.wants_keyboard_input() {
        if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) { go_up = true; }
    }
    if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) { go_back = true; }
    if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) { go_fwd = true; }
    if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::F)) { st.picker_focus_filter = true; }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) { out.close = true; }

    if reload { st.reload_picker(); }
    if path_submit {
        let input = st.picker_path_input.trim().to_string();
        let base = st.picker_cwd.clone();
        if let Some(dir) = expand_and_validate_in(&input, &base) { goto = Some(dir); }
        else if Path::new(&input).is_file() { out.choose = Some(PathBuf::from(&input)); }
        else { st.picker_path_input = st.picker_cwd.display().to_string(); }
    }
    if let Some(i) = activate {
        if let Some(e) = st.picker_entries.get(i) {
            if e.is_dir { goto = Some(e.path.clone()); } else { out.choose = Some(e.path.clone()); }
        }
    }
    if go_back { st.picker_back(); }
    if go_fwd { st.picker_forward(); }
    if go_up { st.picker_up(); }
    if let Some(p) = goto { st.picker_goto(p); }
    out
}