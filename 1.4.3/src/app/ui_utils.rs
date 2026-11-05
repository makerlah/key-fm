use eframe::egui;
use crate::theme::Theme;
use crate::constants::{THIS_PC, RECYCLE, QUICK_ACCESS, VIRTUAL};
use crate::helpers::expand_and_validate_in;
use std::env;
use std::path::{Path, PathBuf};

pub(crate) const TOPBAR_ROW_H: f32 = 28.0;
pub(crate) const PANEL_PAD_X: f32 = 8.0;
pub(crate) const PANEL_PAD_TOP: f32 = 4.0;

pub(crate) fn sel_fill(theme: &Theme, _ui: &egui::Ui) -> egui::Color32 {
    theme.sel_bg
}

pub(crate) fn stroke_rect(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    painter.line_segment([rect.left_top(), rect.right_top()], stroke);
    painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
    painter.line_segment([rect.right_bottom(), rect.left_bottom()], stroke);
    painter.line_segment([rect.left_bottom(), rect.left_top()], stroke);
}

pub(crate) fn draw_icon(ui: &mut egui::Ui, ppp: f32, tex: Option<&egui::TextureHandle>, is_dir: bool) {
    let size = tex.map(|t| t.size_vec2() / ppp).unwrap_or_else(|| egui::vec2(16.0, 16.0));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    match tex {
        Some(t) => {
            ui.painter().image(
                t.id(), rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None if is_dir => {
            ui.painter().rect_filled(
                rect.shrink2(egui::vec2(1.0, 3.0)), 2.0,
                egui::Color32::from_rgb(0xf0, 0xc0, 0x4a),
            );
        }
        None => {}
    }
}

pub(crate) fn location_label(loc: &str) -> String {
    if loc == THIS_PC      { return "This PC".to_string(); }
    if loc == RECYCLE      { return "Recycle Bin".to_string(); }
    if loc == QUICK_ACCESS { return "Quick access".to_string(); }
    Path::new(loc)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| loc.to_string())
}

pub(crate) fn parse_virtual(raw: &str) -> Option<&'static str> {
    let l = raw.trim().to_lowercase();
    if l == THIS_PC || l == "this pc" || l == "this-pc" { Some(THIS_PC) }
    else if l == RECYCLE || l == "recycle bin" || l == "recycle-bin" { Some(RECYCLE) }
    else if l == QUICK_ACCESS || l == "quick access" || l == "quick-access" { Some(QUICK_ACCESS) }
    else { None }
}

pub(crate) fn resolve_location(raw: &str, cwd: &str) -> Option<String> {
    if let Some(v) = parse_virtual(raw) {
        return Some(v.to_string());
    }
    let base = if VIRTUAL.contains(&cwd) {
        env::current_dir().unwrap_or_default()
    } else {
        PathBuf::from(cwd)
    };
    expand_and_validate_in(raw, &base).map(|p| p.to_string_lossy().to_string())
}

pub(crate) fn loc_ok(loc: &str) -> bool {
    VIRTUAL.contains(&loc) || (Path::new(loc).is_dir() && std::fs::read_dir(loc).is_ok())
}