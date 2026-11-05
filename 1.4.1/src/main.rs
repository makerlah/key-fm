use eframe::egui;
use std::env;
use keyfm::config::Config;
use keyfm::theme::{LIGHT, DARK, Theme};
use keyfm::helpers::{get_start_dir, expand_and_validate_in, same_loc};
use keyfm::constants::{THIS_PC, RECYCLE, VIRTUAL, title_for, pc_title_for, bin_title_for};

use keyfm::icons::{IconCache, IconKind};
use keyfm::models::{Item, Tab};
use keyfm::drives;
use keyfm::helpers::{human_size, fmt_time};
use std::path::{Path, PathBuf};

/// Shared height of the menus row and the toolbar row so their
/// horizontal separators land on the same Y.
const TOPBAR_ROW_H: f32 = 28.0;
/// Horizontal padding inside each top-row / sidebar.
const PANEL_PAD_X: f32 = 8.0;
/// Space above the top row (inside the panel).
const PANEL_PAD_TOP: f32 = 4.0;

fn sel_fill(theme: &Theme, _ui: &egui::Ui) -> egui::Color32 {
    theme.sel_bg
}

fn stroke_rect(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    painter.line_segment([rect.left_top(), rect.right_top()], stroke);
    painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
    painter.line_segment([rect.right_bottom(), rect.left_bottom()], stroke);
    painter.line_segment([rect.left_bottom(), rect.left_top()], stroke);
}

fn draw_icon(ui: &mut egui::Ui, ppp: f32, tex: Option<&egui::TextureHandle>, is_dir: bool) {
    let size = tex
        .map(|t| t.size_vec2() / ppp)
        .unwrap_or_else(|| egui::vec2(16.0, 16.0));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    match tex {
        Some(t) => {
            ui.painter().image(
                t.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None if is_dir => {
            ui.painter().rect_filled(
                rect.shrink2(egui::vec2(1.0, 3.0)),
                2.0,
                egui::Color32::from_rgb(0xf0, 0xc0, 0x4a),
            );
        }
        None => {}
    }
}

fn location_label(loc: &str) -> String {
    if loc == THIS_PC {
        "This PC".to_string()
    } else if loc == RECYCLE {
        "Recycle Bin".to_string()
    } else {
        Path::new(loc)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| loc.to_string())
    }
}

fn parse_virtual(raw: &str) -> Option<&'static str> {
    let l = raw.trim().to_lowercase();
    if l == THIS_PC || l == "this pc" || l == "this-pc" {
        Some(THIS_PC)
    } else if l == RECYCLE || l == "recycle bin" || l == "recycle-bin" {
        Some(RECYCLE)
    } else {
        None
    }
}

fn resolve_location(raw: &str, cwd: &str) -> Option<String> {
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

fn loc_ok(loc: &str) -> bool {
    VIRTUAL.contains(&loc) || (Path::new(loc).is_dir() && std::fs::read_dir(loc).is_ok())
}

#[derive(Clone)]
struct ClipItem {
    path: PathBuf,
    name: String,
}

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
    NewFolder,
    NewFile,
    Refresh,
}

// ---------- Open With support ----------

struct PickerEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    size: u64,
    mtime: Option<f64>,
}

struct OpenWithState {
    target: PathBuf,
    filter: String,
    focus_filter: bool,
    // "Choose another program" window
    picker_open: bool,
    picker_cwd: PathBuf,
    picker_entries: Vec<PickerEntry>,
    picker_selected: Option<usize>,
    picker_filter: String,
    picker_path_input: String,
    picker_show_all: bool,
    picker_hist: Vec<PathBuf>,
    picker_fwd: Vec<PathBuf>,
    picker_scroll_to: Option<usize>,
    picker_set_offset: Option<f32>,
    picker_focus_filter: bool,
}

impl OpenWithState {
    fn new(target: PathBuf) -> Self {
        Self {
            target,
            filter: String::new(),
            focus_filter: true,
            picker_open: false,
            picker_cwd: PathBuf::new(),
            picker_entries: Vec::new(),
            picker_selected: None,
            picker_filter: String::new(),
            picker_path_input: String::new(),
            picker_show_all: false,
            picker_hist: Vec::new(),
            picker_fwd: Vec::new(),
            picker_scroll_to: None,
            picker_set_offset: None,
            picker_focus_filter: false,
        }
    }

    fn start_picker(&mut self) {
        self.picker_open = true;
        self.picker_hist.clear();
        self.picker_fwd.clear();
        self.picker_filter.clear();
        self.picker_cwd = default_picker_dir();
        self.reload_picker();
    }

    fn reload_picker(&mut self) {
        self.picker_entries.clear();
        self.picker_selected = None;
        self.picker_scroll_to = None;
        self.picker_set_offset = Some(0.0);
        self.picker_path_input = self.picker_cwd.display().to_string();
        let Ok(rd) = std::fs::read_dir(&self.picker_cwd) else {
            return;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().ok();
            let is_dir = path.is_dir();
            let hidden = (cfg!(unix) && name.starts_with('.'))
                || keyfm::helpers::is_windows_hidden(meta.as_ref());
            if hidden && !self.picker_show_all {
                continue;
            }
            if !is_dir && !self.picker_show_all && !is_executable_candidate(&path) {
                continue;
            }
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64());
            self.picker_entries.push(PickerEntry { name, path, is_dir, size, mtime });
        }
        self.picker_entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
    }

    fn picker_goto(&mut self, p: PathBuf) {
        if p != self.picker_cwd {
            let old = std::mem::replace(&mut self.picker_cwd, p);
            if !old.as_os_str().is_empty() {
                self.picker_hist.push(old);
            }
            self.picker_fwd.clear();
        }
        self.picker_filter.clear();
        self.reload_picker();
    }

    fn picker_back(&mut self) {
        if let Some(p) = self.picker_hist.pop() {
            let old = std::mem::replace(&mut self.picker_cwd, p);
            self.picker_fwd.push(old);
            self.picker_filter.clear();
            self.reload_picker();
        }
    }

    fn picker_forward(&mut self) {
        if let Some(p) = self.picker_fwd.pop() {
            let old = std::mem::replace(&mut self.picker_cwd, p);
            self.picker_hist.push(old);
            self.picker_filter.clear();
            self.reload_picker();
        }
    }

    fn picker_up(&mut self) {
        if let Some(parent) = self.picker_cwd.parent().map(|p| p.to_path_buf()) {
            self.picker_goto(parent);
        }
    }
}

/// Where the "Choose another program" browser starts (not keyfm's own folder).
fn default_picker_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(p) = std::env::var_os("ProgramFiles") {
            let p = PathBuf::from(p);
            if p.is_dir() {
                return p;
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let p = PathBuf::from("/Applications");
        if p.is_dir() {
            return p;
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let p = PathBuf::from("/usr/bin");
        if p.is_dir() {
            return p;
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn is_executable_candidate(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        matches!(
            ext.as_str(),
            "exe" | "bat" | "cmd" | "com"
        )
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
}

struct PickerOutcome {
    choose: Option<PathBuf>,
    close: bool,
}

const PICKER_ROW_H: f32 = 24.0;

/// Quick-access places shown in the program chooser's sidebar.
fn picker_shortcuts(known: &[(&'static str, PathBuf)]) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = Vec::new();
    #[cfg(windows)]
    {
        for (label, var, sub) in [
            ("Program Files", "ProgramFiles", ""),
            ("Program Files (x86)", "ProgramFiles(x86)", ""),
            ("Local programs", "LOCALAPPDATA", "Programs"),
        ] {
            if let Some(base) = std::env::var_os(var) {
                let mut p = PathBuf::from(base);
                if !sub.is_empty() {
                    p.push(sub);
                }
                v.push((label.to_string(), p));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        v.push(("Applications".to_string(), PathBuf::from("/Applications")));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        v.push(("/usr/bin".to_string(), PathBuf::from("/usr/bin")));
        v.push(("/usr/local/bin".to_string(), PathBuf::from("/usr/local/bin")));
        v.push(("/opt".to_string(), PathBuf::from("/opt")));
    }
    for (label, path) in known {
        v.push((label.to_string(), path.clone()));
    }
    v
}

/// The body of the "Open with…" program chooser, drawn in its own OS window.
fn program_picker_ui(
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

    // Rows that pass the live filter (indices into st.picker_entries).
    let needle = st.picker_filter.trim().to_lowercase();
    let visible: Vec<usize> = st
        .picker_entries
        .iter()
        .enumerate()
        .filter(|(_, e)| needle.is_empty() || e.name.to_lowercase().contains(&needle))
        .map(|(i, _)| i)
        .collect();

    // ---- top bar: navigation, path, filter ----
    egui::TopBottomPanel::top("pk_top").show_inside(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!st.picker_hist.is_empty(), egui::Button::new("←"))
                .on_hover_text("Back (Alt+Left)")
                .clicked()
            {
                go_back = true;
            }
            if ui
                .add_enabled(!st.picker_fwd.is_empty(), egui::Button::new("→"))
                .on_hover_text("Forward (Alt+Right)")
                .clicked()
            {
                go_fwd = true;
            }
            if ui.button("↑").on_hover_text("Up (Backspace)").clicked() {
                go_up = true;
            }
            let w = (ui.available_width() - 200.0).max(120.0);
            let r = ui.add(egui::TextEdit::singleline(&mut st.picker_path_input).desired_width(w));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                path_submit = true;
            }
            let f = ui.add(
                egui::TextEdit::singleline(&mut st.picker_filter)
                    .hint_text("Filter (Ctrl+F)")
                    .desired_width(f32::INFINITY),
            );
            if st.picker_focus_filter {
                f.request_focus();
                st.picker_focus_filter = false;
            }
        });
        ui.add_space(4.0);
    });

    // ---- bottom bar: selection + buttons ----
    let sel_file: Option<PathBuf> = st
        .picker_selected
        .and_then(|i| st.picker_entries.get(i))
        .filter(|e| !e.is_dir)
        .map(|e| e.path.clone());
    egui::TopBottomPanel::bottom("pk_bottom").show_inside(ui, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let shown = sel_file
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "select a program".to_string());
            ui.label(format!("Open \"{}\" with:", target_name));
            ui.label(egui::RichText::new(shown).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    out.close = true;
                }
                if ui
                    .add_enabled(sel_file.is_some(), egui::Button::new("Open"))
                    .clicked()
                {
                    out.choose = sel_file.clone();
                }
                if ui.checkbox(&mut st.picker_show_all, "All files").changed() {
                    reload = true;
                }
            });
        });
        ui.add_space(6.0);
    });

    // ---- sidebar ----
    let shortcuts = picker_shortcuts(known);
    egui::SidePanel::left("pk_side")
        .resizable(true)
        .default_width(190.0)
        .width_range(140.0..=320.0)
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(6.0);
                ui.weak("Quick access");
                for (label, path) in &shortcuts {
                    if !path.is_dir() {
                        continue;
                    }
                    let cur = same_loc(&st.picker_cwd.to_string_lossy(), &path.to_string_lossy());
                    ui.horizontal(|ui| {
                        let tex = icons.get(ctx, IconKind::Folder);
                        draw_icon(ui, ppp, tex.as_ref(), true);
                        if ui.selectable_label(cur, label.as_str()).clicked() {
                            goto = Some(path.clone());
                        }
                    });
                }
                ui.add_space(8.0);
                ui.weak("Drives");
                for (label, path, _, _) in drives {
                    let cur = same_loc(&st.picker_cwd.to_string_lossy(), &path.to_string_lossy());
                    ui.horizontal(|ui| {
                        let tex = icons.get(ctx, IconKind::Drive(path));
                        draw_icon(ui, ppp, tex.as_ref(), false);
                        if ui.selectable_label(cur, label.as_str()).clicked() {
                            goto = Some(path.clone());
                        }
                    });
                }
            });
        });

    // ---- file list ----
    egui::CentralPanel::default().show_inside(ui, |ui| {
        let font = egui::TextStyle::Body.resolve(ui.style());

        // header
        let (hr, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 22.0),
            egui::Sense::hover(),
        );
        let size_right = hr.right() - 14.0;
        let mod_left = size_right - 90.0 - 150.0;
        let hp = ui.painter();
        hp.text(
            egui::pos2(hr.left() + 30.0, hr.center().y),
            egui::Align2::LEFT_CENTER,
            "Name",
            font.clone(),
            theme.dim,
        );
        hp.text(
            egui::pos2(mod_left, hr.center().y),
            egui::Align2::LEFT_CENTER,
            "Date modified",
            font.clone(),
            theme.dim,
        );
        hp.text(
            egui::pos2(size_right, hr.center().y),
            egui::Align2::RIGHT_CENTER,
            "Size",
            font.clone(),
            theme.dim,
        );
        hp.line_segment(
            [hr.left_bottom(), hr.right_bottom()],
            egui::Stroke::new(1.0_f32, theme.border),
        );

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
        if let Some(off) = st.picker_set_offset.take() {
            sa = sa.vertical_scroll_offset(off);
        }
        let res = sa.show_rows(ui, PICKER_ROW_H, visible.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in range {
                let i = visible[row];
                let (name, path, is_dir, size, mtime) = {
                    let e = &st.picker_entries[i];
                    (e.name.clone(), e.path.clone(), e.is_dir, e.size, e.mtime)
                };
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), PICKER_ROW_H),
                    egui::Sense::click(),
                );
                let selected = st.picker_selected == Some(i);
                let fg = if selected { theme.sel_fg } else { ui.visuals().text_color() };
                let dim = if selected { theme.sel_fg } else { theme.dim };
                let kind = if is_dir {
                    IconKind::Folder
                } else {
                    IconKind::File { name: &name, path: &path }
                };
                let tex = icons.get(ctx, kind);

                let painter = ui.painter();
                if selected {
                    painter.rect_filled(rect, 3.0, theme.sel_bg);
                } else if resp.hovered() {
                    painter.rect_filled(rect, 3.0, theme.hover);
                }

                let icon_size = tex
                    .as_ref()
                    .map(|t| t.size_vec2() / ppp)
                    .unwrap_or_else(|| egui::vec2(16.0, 16.0));
                let icon_rect = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 16.0, rect.center().y),
                    icon_size,
                );
                match tex.as_ref() {
                    Some(t) => {
                        painter.image(
                            t.id(),
                            icon_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    None if is_dir => {
                        painter.rect_filled(
                            icon_rect.shrink2(egui::vec2(1.0, 3.0)),
                            2.0,
                            egui::Color32::from_rgb(0xf0, 0xc0, 0x4a),
                        );
                    }
                    None => {}
                }

                let r_size = rect.right() - 14.0;
                let r_mod = r_size - 90.0 - 150.0;
                let name_clip = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 30.0, rect.top()),
                    egui::pos2(r_mod - 8.0, rect.bottom()),
                );
                painter.with_clip_rect(name_clip).text(
                    egui::pos2(rect.left() + 30.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &name,
                    font.clone(),
                    fg,
                );
                painter.text(
                    egui::pos2(r_mod, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    fmt_time(mtime),
                    font.clone(),
                    dim,
                );
                if !is_dir {
                    painter.text(
                        egui::pos2(r_size, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        human_size(size),
                        font.clone(),
                        dim,
                    );
                }

                if resp.clicked() {
                    st.picker_selected = Some(i);
                }
                if resp.double_clicked() {
                    activate = Some(i);
                }
            }
        });

        // Keep a keyboard-selected row in view (rows off-screen are not drawn).
        if let Some(pos) = st.picker_scroll_to.take() {
            let top = res.state.offset.y;
            let h = res.inner_rect.height();
            let row_top = pos as f32 * PICKER_ROW_H;
            if row_top < top {
                st.picker_set_offset = Some(row_top);
                ctx.request_repaint();
            } else if row_top + PICKER_ROW_H > top + h {
                st.picker_set_offset = Some(row_top + PICKER_ROW_H - h);
                ctx.request_repaint();
            }
        }
    });

    // ---- keyboard ----
    if !ctx.wants_keyboard_input() && !visible.is_empty() {
        let cur_pos = st
            .picker_selected
            .and_then(|s| visible.iter().position(|&v| v == s));
        let last = visible.len() - 1;
        let mut new_pos: Option<usize> = None;
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            new_pos = Some(cur_pos.map_or(0, |p| (p + 1).min(last)));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            new_pos = Some(cur_pos.map_or(0, |p| p.saturating_sub(1)));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
            new_pos = Some(0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::End)) {
            new_pos = Some(last);
        }
        if let Some(np) = new_pos {
            st.picker_selected = Some(visible[np]);
            st.picker_scroll_to = Some(np);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(s) = st.picker_selected {
                activate = Some(s);
            }
        }
    }
    if !ctx.wants_keyboard_input() {
        if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) {
            go_up = true;
        }
    }
    if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) {
        go_back = true;
    }
    if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) {
        go_fwd = true;
    }
    if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::F)) {
        st.picker_focus_filter = true;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        out.close = true;
    }

    // ---- apply what happened this frame ----
    if reload {
        st.reload_picker();
    }
    if path_submit {
        let input = st.picker_path_input.trim().to_string();
        let base = st.picker_cwd.clone();
        if let Some(dir) = expand_and_validate_in(&input, &base) {
            goto = Some(dir);
        } else if Path::new(&input).is_file() {
            out.choose = Some(PathBuf::from(&input));
        } else {
            st.picker_path_input = st.picker_cwd.display().to_string();
        }
    }
    if let Some(i) = activate {
        if let Some(e) = st.picker_entries.get(i) {
            if e.is_dir {
                goto = Some(e.path.clone());
            } else {
                out.choose = Some(e.path.clone());
            }
        }
    }
    if go_back {
        st.picker_back();
    }
    if go_fwd {
        st.picker_forward();
    }
    if go_up {
        st.picker_up();
    }
    if let Some(p) = goto {
        st.picker_goto(p);
    }
    out
}

struct KeyFmApp {
    config: Config,
    theme: Theme,
    cwd: String,
    entries: Vec<Item>,
    selected_item: Option<usize>,
    search_query: String,
    path_input: String,
    tabs: Vec<Tab>,
    cur_tab: usize,
    clipboard: Vec<ClipItem>,
    clip_mode: String,
    active_dialog: Option<(String, String)>,
    lasso_start: Option<egui::Pos2>,
    selected_items: Vec<usize>,
    status_msg: String,
    confirm_empty_bin: bool,
    dialog_seen: bool,
    icons: IconCache,
    known_cache: Vec<(&'static str, PathBuf)>,
    drives_info_cache: Vec<(String, PathBuf, u64, u64)>,
    search_rx: Option<std::sync::mpsc::Receiver<SearchHit>>,
    search_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    search_threads: Option<Vec<std::thread::JoinHandle<()>>>,
    search_debounce: Option<std::time::Instant>,
    search_results: Vec<Item>,
    search_running: bool,
    search_truncated: bool,
    last_search_query: String,
    show_shortcuts: bool,
    open_with: Option<OpenWithState>,
}

impl KeyFmApp {
    fn new(start: Option<String>) -> Self {
        let (config, config_notice) = keyfm::config::load_config_status();
        let mut config = config;
        if config.sidebar_width < 120.0 {
            config.sidebar_width = keyfm::config::default_sidebar_width();
        }
        if config.size_col_width < 60.0 {
            config.size_col_width = keyfm::config::default_size_col();
        }
        if config.mod_col_width < 60.0 {
            config.mod_col_width = keyfm::config::default_mod_col();
        }
        let theme = if config.dark { DARK } else { LIGHT };
        let start_dir = get_start_dir(start);
        let cwd = start_dir.to_string_lossy().to_string();
        let path_input = cwd.clone();
        let tab = Tab::new(Some(start_dir.clone()));
        let mut app = Self {
            config,
            show_shortcuts: false,
            theme,
            cwd,
            entries: Vec::new(),
            selected_item: None,
            search_query: String::new(),
            path_input,
            tabs: vec![tab],
            cur_tab: 0,
            clipboard: Vec::new(),
            clip_mode: "copy".to_string(),
            active_dialog: None,
            lasso_start: None,
            selected_items: Vec::new(),
            status_msg: String::new(),
            confirm_empty_bin: false,
            dialog_seen: false,
            icons: IconCache::new(),
            known_cache: drives::known_folders(),
            drives_info_cache: drives::scan_drives_info(),
            search_rx: None,
            search_cancel: None,
            search_threads: None,
            search_debounce: None,
            search_results: Vec::new(),
            search_running: false,
            search_truncated: false,
            last_search_query: String::new(),
            open_with: None,
        };
        app.refresh_dir();
        if let Some(n) = config_notice {
            app.status_msg = n;
        }
        app
    }

    fn save_current_tab(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.cur_tab) {
            tab.cwd = Some(std::path::PathBuf::from(&self.cwd));
            tab.search_query = self.search_query.clone();
            tab.focus = self
                .selected_item
                .and_then(|idx| self.entries.get(idx))
                .map(|item| item.path.clone());
        }
    }

    fn load_tab(&mut self, index: usize) {
        self.save_current_tab();
        self.load_tab_no_save(index);
    }

    fn load_tab_no_save(&mut self, index: usize) {
        self.cancel_search();
        self.search_query.clear();
        self.last_search_query.clear();
        if index < self.tabs.len() {
            let tab = &self.tabs[index];
            let new_cwd = tab.cwd.clone();
            let new_query = tab.search_query.clone();
            let new_focus = tab.focus.clone();

            if let Some(cwd) = new_cwd {
                self.cwd = cwd.to_string_lossy().to_string();
            }
            self.search_query = new_query;
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
        self.close_tab_at(self.cur_tab);
    }

    fn close_tab_at(&mut self, idx: usize) {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() {
            return;
        }
        self.save_current_tab();
        self.tabs.remove(idx);
        if idx < self.cur_tab {
            self.cur_tab -= 1;
        } else if idx == self.cur_tab {
            if self.cur_tab >= self.tabs.len() {
                self.cur_tab = self.tabs.len() - 1;
            }
            let target = self.cur_tab;
            self.load_tab_no_save(target);
        }
    }

    fn set_cwd(&mut self, new_cwd: String) {
        self.status_msg.clear();
        self.cwd = new_cwd;
        if let Some(t) = self.tabs.get_mut(self.cur_tab) {
            t.cwd = Some(std::path::PathBuf::from(&self.cwd));
        }
        self.refresh_dir();
    }

    fn is_virtual_view(&self) -> bool {
        VIRTUAL.contains(&self.cwd.as_str())
    }

    fn cwd_display(&self) -> String {
        if self.is_virtual_view() {
            location_label(&self.cwd)
        } else {
            self.cwd.clone()
        }
    }

    fn parent_location(&self) -> Option<String> {
        if self.cwd == THIS_PC {
            return None;
        }
        if self.cwd == RECYCLE {
            return Some(THIS_PC.to_string());
        }
        match Path::new(&self.cwd).parent() {
            Some(par) if !par.as_os_str().is_empty() => Some(par.to_string_lossy().to_string()),
            _ if cfg!(windows) => Some(THIS_PC.to_string()),
            _ => None,
        }
    }

    fn go_to(&mut self, new_loc: String) {
        self.cancel_search();
        self.search_query.clear();
        self.last_search_query.clear();
        if !same_loc(&new_loc, &self.cwd) {
            let cur = PathBuf::from(&self.cwd);
            if let Some(t) = self.tabs.get_mut(self.cur_tab) {
                t.back.push(cur);
                if t.back.len() > 200 {
                    t.back.remove(0);
                }
                t.fwd.clear();
            }
        }
        self.set_cwd(new_loc);
    }

    fn pop_valid(&mut self, forward: bool) -> (Option<PathBuf>, Option<String>) {
        let mut skipped: Option<String> = None;
        loop {
            let popped = self.tabs.get_mut(self.cur_tab).and_then(|t| {
                if forward {
                    t.fwd.pop()
                } else {
                    t.back.pop()
                }
            });
            match popped {
                Some(c) => {
                    let s = c.to_string_lossy().to_string();
                    if loc_ok(&s) {
                        return (Some(c), skipped);
                    }
                    skipped = Some(s);
                }
                None => return (None, skipped),
            }
        }
    }

    fn go_back(&mut self) {
        self.cancel_search();
        self.search_query.clear();
        self.last_search_query.clear();
        let (prev, skipped) = self.pop_valid(false);
        if let Some(prev) = prev {
            let cur = PathBuf::from(&self.cwd);
            if let Some(t) = self.tabs.get_mut(self.cur_tab) {
                t.fwd.push(cur);
            }
            self.set_cwd(prev.to_string_lossy().to_string());
        }
        if let Some(s) = skipped {
            self.status_msg = format!("Folder no longer available: {}", s);
        }
    }

    fn go_forward(&mut self) {
        self.cancel_search();
        self.search_query.clear();
        self.last_search_query.clear();
        let (next, skipped) = self.pop_valid(true);
        if let Some(next) = next {
            let cur = PathBuf::from(&self.cwd);
            if let Some(t) = self.tabs.get_mut(self.cur_tab) {
                t.back.push(cur);
            }
            self.set_cwd(next.to_string_lossy().to_string());
        }
        if let Some(s) = skipped {
            self.status_msg = format!("Folder no longer available: {}", s);
        }
    }

    fn load_this_pc(&mut self) {
        for (label, mount, free, total) in drives::scan_drives_info() {
            self.entries.push(Item {
                name: label,
                path: mount,
                is_dir: true,
                size: free,
                mtime: None,
                hidden: false,
                extra: String::new(),
                orig: None,
                info: String::new(),
                total,
            });
        }
    }

    fn load_recycle(&mut self) {
        if !cfg!(windows) {
            self.status_msg = "The Recycle Bin view is only available on Windows".to_string();
            return;
        }
        for e in keyfm::trash::list_recycle_bin() {
            self.entries.push(Item {
                name: e.name,
                path: e.path,
                is_dir: e.is_dir,
                size: e.size,
                mtime: e.deleted,
                hidden: false,
                extra: String::new(),
                orig: Some(PathBuf::from(e.original)),
                info: String::new(),
                total: 0,
            });
        }
    }

    fn modal_open(&self) -> bool {
        self.active_dialog.is_some()
            || self.confirm_empty_bin
            || self.show_shortcuts
            || self.open_with.is_some()
    }

    fn rendered_entries(&self) -> &[Item] {
        if self.search_active() {
            &self.search_results
        } else {
            &self.entries
        }
    }

    fn search_active(&self) -> bool {
        self.search_query.chars().count() > 1
            && (self.search_running || !self.search_results.is_empty())
    }

    fn is_visible(&self, idx: usize) -> bool {
        let f = self.search_query.to_lowercase();
        f.is_empty()
            || self
                .rendered_entries()
                .get(idx)
                .map_or(false, |e| e.name.to_lowercase().contains(&f))
    }

    fn visible_indices(&self) -> Vec<usize> {
        (0..self.rendered_entries().len())
            .filter(|&i| self.is_visible(i))
            .collect()
    }

    fn targets(&self) -> Vec<usize> {
        let mut t: Vec<usize> = if !self.selected_items.is_empty() {
            self.selected_items.clone()
        } else {
            self.selected_item.into_iter().collect()
        };
        t.sort();
        t.dedup();
        t.retain(|&i| self.is_visible(i));
        t
    }

    fn copy_or_cut(&mut self, mode: &str) {
        if self.cwd == THIS_PC || (self.cwd == RECYCLE && mode == "cut") {
            self.status_msg = "Not available in this view".to_string();
            return;
        }
        let t = self.targets();
        if !t.is_empty() {
            self.clipboard.clear();
            let src: &[Item] = if self.search_active() {
                &self.search_results
            } else {
                &self.entries
            };
            for idx in t {
                if let Some(item) = src.get(idx) {
                    let name = item
                        .orig
                        .as_ref()
                        .and_then(|o| o.file_name())
                        .or_else(|| item.path.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| item.name.clone());
                    self.clipboard.push(ClipItem {
                        path: item.path.clone(),
                        name,
                    });
                }
            }
            self.clip_mode = mode.to_string();
        }
    }

    fn paste(&mut self) {
        if self.is_virtual_view() {
            self.status_msg = "Cannot paste into this view".to_string();
            return;
        }
        if self.clipboard.is_empty() {
            return;
        }
        let dest_dir = PathBuf::from(&self.cwd);
        let cut = self.clip_mode == "cut";
        let items = self.clipboard.clone();

        let mut done = 0usize;
        let mut failed: Vec<ClipItem> = Vec::new();
        let mut missing = 0usize;
        let mut first_err: Option<String> = None;

        for item in &items {
            let src = &item.path;
            let Ok(meta) = src.symlink_metadata() else {
                missing += 1;
                first_err.get_or_insert(format!("\"{}\" no longer exists", item.name));
                continue;
            };
            let same_folder = src
                .parent()
                .map_or(false, |p| same_loc(&p.to_string_lossy(), &self.cwd));
            if cut && same_folder {
                continue;
            }
            let dest = keyfm::helpers::unique_path(&dest_dir, &item.name, meta.is_dir());
            let res = if cut {
                keyfm::helpers::move_path(src, &dest)
            } else {
                keyfm::helpers::copy_any(src, &dest)
            };
            match res {
                Ok(()) => done += 1,
                Err(e) => {
                    first_err.get_or_insert(format!("{}: {}", item.name, e));
                    failed.push(item.clone());
                }
            }
        }

        if cut {
            self.clipboard = failed.clone();
        }
        self.refresh_dir();
        let nfail = failed.len() + missing;
        self.status_msg = match first_err {
            Some(err) if nfail > 0 => format!("Pasted {}, {} failed ({})", done, nfail, err),
            _ => String::new(),
        };
    }

    fn delete_selected(&mut self, permanent: bool) {
        if self.is_virtual_view() {
            self.status_msg = "Cannot delete from this view".to_string();
            return;
        }
        let src: &[Item] = if self.search_active() {
            &self.search_results
        } else {
            &self.entries
        };
        let paths: Vec<PathBuf> = self
            .targets()
            .into_iter()
            .filter_map(|i| src.get(i).map(|e| e.path.clone()))
            .collect();
        if paths.is_empty() {
            return;
        }

        let mut failed: Vec<PathBuf> = Vec::new();
        let mut first_err: Option<String> = None;
        for (i, path) in paths.iter().enumerate() {
            let res = if permanent {
                keyfm::helpers::delete_permanently(path)
            } else {
                keyfm::trash::move_to_trash(path)
            };
            if let Err(e) = res {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    failed.extend(paths[i..].iter().cloned());
                    first_err.get_or_insert("cancelled".to_string());
                    break;
                }
                let what = if permanent { "Delete" } else { "Trash" };
                first_err.get_or_insert(format!(
                    "{} failed for \"{}\": {}",
                    what,
                    path.display(),
                    e
                ));
                failed.push(path.clone());
            }
        }

        self.refresh_dir();
        if !failed.is_empty() {
            let sel: Vec<usize> = self
                .rendered_entries()
                .iter()
                .enumerate()
                .filter(|(_, e)| failed.contains(&e.path))
                .map(|(i, _)| i)
                .collect();
            self.selected_item = sel.first().copied();
            self.selected_items = sel;
            self.status_msg = format!(
                "{} of {} item(s) not deleted: {}",
                failed.len(),
                paths.len(),
                first_err.unwrap_or_default()
            );
        }
    }

    fn cancel_search(&mut self) {
        if let Some(c) = self.search_cancel.take() {
            c.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.search_rx = None;
        if let Some(ts) = self.search_threads.take() {
            for t in ts {
                let _ = t.join();
            }
        }
        self.search_running = false;
        self.search_results.clear();
        self.search_truncated = false;
        self.search_debounce = None;
    }

    fn save_cfg(&mut self) {
        if !keyfm::config::save_config(&self.config) {
            self.status_msg = "Failed to save config".to_string();
        }
    }

    fn toggle_pin(&mut self, path: &str) {
        if let Some(pos) = self.config.pins.iter().position(|p| p == path) {
            self.config.pins.remove(pos);
        } else {
            self.config.pins.push(path.to_string());
        }
        self.save_cfg();
    }

    fn refresh_caches(&mut self) {
        self.known_cache = drives::known_folders();
        self.drives_info_cache = drives::scan_drives_info();
    }

    fn refresh_dir(&mut self) {
        self.reload(false);
    }

    fn refresh_keep_selection(&mut self) {
        self.refresh_caches();
        self.reload(true);
    }

    fn reload(&mut self, keep_selection: bool) {
        self.cancel_search();
        let kept_item: Option<PathBuf> = if keep_selection {
            self.selected_item
                .and_then(|i| self.rendered_entries().get(i))
                .map(|e| e.path.clone())
        } else {
            None
        };
        let kept_multi: Vec<PathBuf> = if keep_selection {
            self.selected_items
                .iter()
                .filter_map(|&i| self.rendered_entries().get(i))
                .map(|e| e.path.clone())
                .collect()
        } else {
            Vec::new()
        };

        self.entries.clear();
        if self.cwd == THIS_PC {
            self.load_this_pc();
        } else if self.cwd == RECYCLE {
            self.load_recycle();
        }
        let is_virtual = self.is_virtual_view();
        let path = Path::new(&self.cwd);
        let read_dir_res = if is_virtual {
            None
        } else {
            Some(std::fs::read_dir(path))
        };
        if let Some(Err(e)) = &read_dir_res {
            self.status_msg = format!("Cannot read {}: {}", self.cwd, e);
        }
        if let Some(Ok(read_dir)) = read_dir_res {
            for entry in read_dir.flatten() {
                let path = entry.path();
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let metadata = entry.metadata().ok();
                let hidden = name.starts_with('.')
                    || keyfm::helpers::is_windows_hidden(metadata.as_ref());
                if !self.config.show_hidden && hidden {
                    continue;
                }
                let is_link = metadata
                    .as_ref()
                    .map_or(false, |m| m.file_type().is_symlink());
                let metadata = if is_link {
                    std::fs::metadata(&path).ok().or(metadata)
                } else {
                    metadata
                };
                let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = metadata
                    .as_ref()
                    .map(|m| if is_dir { 0 } else { m.len() })
                    .unwrap_or(0);
                let mtime = metadata
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .ok()
                            .map(|d| d.as_secs_f64())
                    });

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
                    total: 0,
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
        self.path_input = self.cwd_display();
        self.selected_item = None;
        self.selected_items.clear();

        if keep_selection {
            self.selected_item = kept_item.and_then(|p| self.entries.iter().position(|e| e.path == p));
            self.selected_items = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| kept_multi.contains(&e.path))
                .map(|(i, _)| i)
                .collect();
        }
    }
}

impl eframe::App for KeyFmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut navigate_to: Option<String> = None;
        let mut pending_action = UiAction::None;
        let mut nav_back = false;
        let mut nav_forward = false;
        self.icons.begin_frame();

        // ---- Search lifecycle: debounce, spawn, drain ----
        if self.search_query != self.last_search_query {
            self.selected_item = None;
            self.selected_items.clear();
            if self.search_query.chars().count() <= 1 {
                self.cancel_search();
            } else {
                self.search_debounce = Some(std::time::Instant::now());
            }
            self.last_search_query = self.search_query.clone();
        }

        if let Some(t) = self.search_debounce {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            if t.elapsed().as_millis() >= SEARCH_DEBOUNCE_MS as u128 {
                self.search_debounce = None;
                self.cancel_search();
                let (tx, rx) = std::sync::mpsc::channel::<SearchHit>();
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let needle = self.search_query.to_lowercase();
                let roots: Vec<PathBuf> = if self.cwd == THIS_PC {
                    drives::scan_drives().into_iter().map(|(_, p)| p).collect()
                } else if self.cwd == RECYCLE {
                    Vec::new()
                } else {
                    vec![PathBuf::from(&self.cwd)]
                };
                let mut handles = Vec::new();
                for root in roots {
                    let tx = tx.clone();
                    let cancel = cancel.clone();
                    let needle = needle.clone();
                    let show_hidden = self.config.show_hidden;
                    handles.push(std::thread::spawn(move || {
                        run_walk(root, needle, show_hidden, cancel, tx);
                    }));
                }
                drop(tx);
                self.search_rx = Some(rx);
                self.search_cancel = Some(cancel);
                self.search_threads = Some(handles);
                self.search_running = true;
                self.search_truncated = false;
                self.search_results.clear();
            }
        }

        if self.search_running || self.search_rx.is_some() {
            ctx.request_repaint();
        }

        if let Some(rx) = self.search_rx.take() {
            loop {
                match rx.try_recv() {
                    Ok(hit) => {
                        if self.search_results.len() < SEARCH_RESULT_CAP {
                            self.search_results.push(hit.into_item());
                        } else {
                            self.search_truncated = true;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        self.search_rx = Some(rx);
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.search_running = false;
                        self.search_results.sort_by(|a, b| {
                            b.is_dir
                                .cmp(&a.is_dir)
                                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                        });
                        if let Some(ts) = self.search_threads.take() {
                            for t in ts {
                                let _ = t.join();
                            }
                        }
                        break;
                    }
                }
            }
        }

        // ---- Theme ----
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

                // --- MODERN WINDOWS 11 STYLING ---
        let mut style = (*ctx.style()).clone();
        
        // 1. Rounding (Windows 11 uses 8px for windows/menus, 4px for buttons)
        style.visuals.window_rounding = 8.0.into();
        style.visuals.menu_rounding = 8.0.into();
        style.visuals.widgets.noninteractive.rounding = 4.0.into();
        style.visuals.widgets.inactive.rounding = 4.0.into();
        style.visuals.widgets.hovered.rounding = 4.0.into();
        style.visuals.widgets.active.rounding = 4.0.into();

        // 2. Shadows (Gives depth to dialogs)
        style.visuals.window_shadow = egui::epaint::Shadow {
            offset: egui::vec2(0.0, 8.0),
            blur: 24.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(60),
        };
        style.visuals.popup_shadow = egui::epaint::Shadow {
            offset: egui::vec2(0.0, 4.0),
            blur: 12.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(40),
        };

        // 3. Spacing (Windows 11 feels much less cramped than default egui)
        style.spacing.item_spacing = egui::vec2(8.0, 8.0); // More breathing room
        style.spacing.button_padding = egui::vec2(12.0, 6.0); // Bigger buttons
        style.spacing.interact_size.y = 32.0; // Taller clickable areas (standard Win11 height)

        // 4. Window margins
        style.spacing.window_margin = egui::Margin::same(16.0);
        
        ctx.set_style(style);

        // ---- Panel 1: tab bar ----
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
                        let title = tab
                            .cwd
                            .as_ref()
                            .map(|p| location_label(&p.to_string_lossy()))
                            .unwrap_or_else(|| "Tab".to_string());
                        let is_active = cur_tab == i;
                        let display_text = format!("\u{1F4C1}  {}", title);
                        let font_id = egui::FontId::proportional(13.0);
                        let text_color = if is_active { theme.fg } else { theme.dim };
                        let galley =
                            ui.painter()
                                .layout_no_wrap(display_text, font_id, text_color);
                        let text_w = galley.size().x.min(220.0);
                        let tab_width = (text_w + padding * 2.0 + close_w).max(150.0);
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(tab_width, tab_height),
                            egui::Sense::click(),
                        );
                        let rounding = egui::Rounding {
                            nw: 8.0,
                            ne: 8.0,
                            sw: 0.0,
                            se: 0.0,
                        };
                        let bg = if is_active { theme.tab_sel } else { theme.tab };
                        ui.painter().rect_filled(rect, rounding, bg);
                        if response.hovered() && !is_active {
                            ui.painter().rect_filled(rect, rounding, theme.hover);
                        }
                        let text_pos = egui::pos2(
                            rect.left() + padding,
                            rect.center().y - galley.size().y / 2.0,
                        );
                        ui.painter().galley(text_pos, galley, text_color);
                        if tab_count > 1 {
                            let close_rect = egui::Rect::from_center_size(
                                egui::pos2(rect.right() - close_w / 2.0 - 3.0, rect.center().y),
                                egui::vec2(14.0, 14.0),
                            );
                            let close_hovered = response.hovered()
                                && ui
                                    .input(|i| i.pointer.hover_pos())
                                    .map_or(false, |p| close_rect.contains(p));
                            let close_color = if close_hovered { theme.danger } else { theme.dim };
                            let stroke = egui::Stroke::new(1.5_f32, close_color);
                            let c = close_rect.center();
                            let h = 4.0;
                            ui.painter().line_segment(
                                [egui::pos2(c.x - h, c.y - h), egui::pos2(c.x + h, c.y + h)],
                                stroke,
                            );
                            ui.painter().line_segment(
                                [egui::pos2(c.x + h, c.y - h), egui::pos2(c.x - h, c.y + h)],
                                stroke,
                            );
                            if response.clicked() {
                                let pos = response
                                    .interact_pointer_pos()
                                    .unwrap_or(rect.center());
                                if close_rect.contains(pos) {
                                    close_tab_idx = Some(i);
                                } else if !is_active {
                                    switch_to_tab = Some(i);
                                }
                            }
                        } else if response.clicked() && !is_active {
                            switch_to_tab = Some(i);
                        }
                        if let Some(p) = tab.cwd.as_ref() {
                            response
                                .clone()
                                .on_hover_text(p.to_string_lossy().to_string());
                        }
                    }

                    ui.add_space(4.0);
                    let (plus_rect, plus_response) = ui.allocate_exact_size(
                        egui::vec2(plus_w, tab_height),
                        egui::Sense::click(),
                    );
                    if plus_response.hovered() {
                        ui.painter().rect_filled(plus_rect, 6.0, theme.hover);
                    }
                    let plus_color = if plus_response.hovered() {
                        theme.fg
                    } else {
                        theme.dim
                    };
                    let c = plus_rect.center();
                    let h = 5.0;
                    let stroke = egui::Stroke::new(1.5_f32, plus_color);
                    ui.painter().line_segment(
                        [egui::pos2(c.x - h, c.y), egui::pos2(c.x + h, c.y)],
                        stroke,
                    );
                    ui.painter().line_segment(
                        [egui::pos2(c.x, c.y - h), egui::pos2(c.x, c.y + h)],
                        stroke,
                    );
                    if plus_response.clicked() {
                        new_tab_clicked = true;
                    }

                    if let Some(i) = close_tab_idx {
                        self.close_tab_at(i);
                    }
                    if let Some(tab_idx) = switch_to_tab {
                        if tab_idx != self.cur_tab {
                            self.load_tab(tab_idx);
                        }
                    }
                    if new_tab_clicked {
                        self.new_tab();
                    }
                });
                ui.add_space(4.0);
            });

        // ---- Panel 2: sidebar ----
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
                        if ui.button("New Tab").clicked() {
                            self.new_tab();
                            ui.close_menu();
                        }
                        if ui.button("Close Tab").clicked() {
                            self.close_tab();
                            ui.close_menu();
                        }
                        if ui.button("New Folder").clicked() {
                            self.active_dialog =
                                Some(("NewFolder".to_string(), "".to_string()));
                            ui.close_menu();
                        }
                        if ui.button("New File").clicked() {
                            self.active_dialog = Some(("NewFile".to_string(), "".to_string()));
                            ui.close_menu();
                        }
                        if ui.button("Empty Recycle Bin").clicked() {
                            self.confirm_empty_bin = true;
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
                                    self.active_dialog =
                                        Some(("Rename".to_string(), item.name.clone()));
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
                    ui.menu_button("View", |ui| {
                        if ui
                            .checkbox(&mut self.config.dark, "Dark mode (Ctrl+Shift+D)")
                            .changed()
                        {
                            self.theme = if self.config.dark { DARK } else { LIGHT };
                            self.save_cfg();
                        }
                        if ui
                            .checkbox(&mut self.config.show_hidden, "Show hidden files")
                            .changed()
                        {
                            self.save_cfg();
                            self.refresh_keep_selection();
                        }
                        ui.separator();
                        if ui.button("Keyboard Shortcuts (F1)").clicked() {
                            self.show_shortcuts = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Refresh").clicked() {
                            self.refresh_keep_selection();
                            ui.close_menu();
                        }
                    });
                });
                let menu_row_bottom = menu_row_top + TOPBAR_ROW_H;

                let sep = egui::Stroke::new(1.0_f32, self.theme.border);
                ui.painter().line_segment(
                    [
                        egui::pos2(panel_rect.left(), menu_row_bottom),
                        egui::pos2(panel_rect.right(), menu_row_bottom),
                    ],
                    sep,
                );
                ui.painter().line_segment(
                    [
                        egui::pos2(panel_rect.right() - 0.5, panel_rect.top()),
                        egui::pos2(panel_rect.right() - 0.5, panel_rect.bottom()),
                    ],
                    sep,
                );

                ui.add_space(10.0);
                ui.label(egui::RichText::new("Quick access").strong().color(self.theme.dim));

                if !self.config.pins.is_empty() {
                    for pin in &self.config.pins {
                        let label = std::path::Path::new(pin)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| pin.clone());
                        let pin_path = std::path::PathBuf::from(pin);
                        let is_selected = same_loc(&self.cwd, pin);
                        let resp = ui.selectable_label(is_selected, format!("📌 {}", label));
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
                    ui.add_space(4.0);
                }

                let known_cache = self.known_cache.clone();
                for (label, path) in &known_cache {
                    let path_str = path.to_string_lossy().to_string();
                    let is_selected = same_loc(&self.cwd, &path_str);
                    let resp = ui.selectable_label(is_selected, *label);
                    if resp.clicked() {
                        navigate_to = Some(path_str.clone());
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Open").clicked() {
                            navigate_to = Some(path_str.clone());
                            ui.close_menu();
                        }
                        if !VIRTUAL.contains(&path_str.as_str()) {
                            if ui.button("Pin").clicked() {
                                pending_action =
                                    UiAction::SidebarPin(std::path::PathBuf::from(&path_str));
                                ui.close_menu();
                            }
                            if ui.button("Unpin").clicked() {
                                pending_action =
                                    UiAction::SidebarUnpin(std::path::PathBuf::from(&path_str));
                                ui.close_menu();
                            }
                        }
                    });
                }

                ui.add_space(8.0);
                let is_pc_selected = self.cwd == THIS_PC;
                if ui
                    .selectable_label(is_pc_selected, egui::RichText::new("This PC").strong())
                    .clicked()
                {
                    navigate_to = Some(THIS_PC.to_string());
                }

                let drives_info_cache = self.drives_info_cache.clone();
                let ppp = ctx.pixels_per_point();
                for (label, path, free, total) in &drives_info_cache {
                    let path_str = path.to_string_lossy().to_string();
                    let is_selected = same_loc(&self.cwd, &path_str);
                    let usage = if *total > 0 {
                        ((*total - *free) as f64 / *total as f64) * 100.0
                    } else {
                        0.0
                    };
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        let tex = self.icons.get(ctx, IconKind::Drive(path));
                        draw_icon(ui, ppp, tex.as_ref(), false);
                        let text = format!("{} {:.1}%", label, usage);
                        if ui.selectable_label(is_selected, text).clicked() {
                            navigate_to = Some(path_str.clone());
                        }
                    });
                }
            });

        let sidebar_width = sidebar_inner.response.rect.width();
        if (sidebar_width - self.config.sidebar_width).abs() > 1.0 {
            self.config.sidebar_width = sidebar_width;
            self.save_cfg();
        }

        // ---- Panel 3: toolbar ----
        let can_back = self
            .tabs
            .get(self.cur_tab)
            .map_or(false, |t| !t.back.is_empty());
        let can_fwd = self
            .tabs
            .get(self.cur_tab)
            .map_or(false, |t| !t.fwd.is_empty());
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

                    if ui
                        .add_enabled(can_back, egui::Button::new("Back"))
                        .clicked()
                    {
                        nav_back = true;
                    }
                    if ui
                        .add_enabled(can_fwd, egui::Button::new("Forward"))
                        .clicked()
                    {
                        nav_forward = true;
                    }
                    if ui.add_enabled(can_up, egui::Button::new("Up")).clicked() {
                        navigate_to = self.parent_location();
                    }
                    ui.label("Path:");
                    let resp = ui.text_edit_singleline(&mut self.path_input);
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        navigate_to = Some(self.path_input.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(PANEL_PAD_X);
                        let search_resp = ui.add_sized(
                            [200.0, 20.0],
                            egui::TextEdit::singleline(&mut self.search_query).hint_text("Search"),
                        );
                        if search_resp.has_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            self.search_query.clear();
                        }
                    });
                });
                let row_bottom = row_top + TOPBAR_ROW_H;

                let sep = egui::Stroke::new(1.0_f32, self.theme.border);
                ui.painter().line_segment(
                    [
                        egui::pos2(panel_rect.left(), row_bottom),
                        egui::pos2(panel_rect.right(), row_bottom),
                    ],
                    sep,
                );
            });

        // ---- F1: shortcuts ----
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_shortcuts = !self.show_shortcuts;
        }

        if self.show_shortcuts {
            let dim_color = self.theme.dim;
            let mut is_open = true;
            let mut close_clicked = false;

            struct Section {
                title: &'static str,
                rows: &'static [(&'static str, &'static str)],
            }
            let sections: &[Section] = &[
                Section {
                    title: "Tabs",
                    rows: &[
                        ("New tab", "Ctrl+T"),
                        ("Close tab", "Ctrl+W"),
                        ("Next tab", "Ctrl+Tab / Ctrl+PgDn"),
                        ("Previous tab", "Ctrl+Shift+Tab / Ctrl+PgUp"),
                        ("Go to tab 1-9", "Alt+1 … Alt+9"),
                    ],
                },
                Section {
                    title: "Files",
                    rows: &[
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
                    ],
                },
                Section {
                    title: "Navigation",
                    rows: &[
                        ("Open / enter folder", "Enter / Right"),
                        ("Up one folder", "Backspace / Left"),
                        ("Back", "Left / Alt+Left"),
                        ("Forward", "Alt+Right"),
                        ("Home folder", "Alt+Home"),
                        ("This PC", "Ctrl+Shift+M"),
                        ("Recycle Bin", "Ctrl+Shift+B"),
                        ("First item", "Home"),
                        ("Last item", "End"),
                        ("Page up", "PgUp"),
                        ("Page down", "PgDn"),
                    ],
                },
                Section {
                    title: "Quick access",
                    rows: &[("Pin / unpin folder", "Ctrl+P")],
                },
                Section {
                    title: "View",
                    rows: &[
                        ("Toggle hidden files", "Ctrl+H"),
                        ("Dark / light mode", "Ctrl+Shift+D"),
                        ("Sort by name", "Ctrl+1"),
                        ("Sort by size", "Ctrl+2"),
                        ("Sort by date", "Ctrl+3"),
                        ("Refresh", "F5 / Ctrl+R"),
                    ],
                },
                Section {
                    title: "Fixed keys",
                    rows: &[
                        ("Up / Down", "Move selection"),
                        ("Shift+Up / Shift+Down", "Extend selection"),
                        ("Space", "Toggle selection"),
                        ("Esc", "Clear / close"),
                        ("Click + drag", "Lasso select"),
                        ("F1", "Toggle this help"),
                        ("Ctrl+Q", "Quit"),
                    ],
                },
            ];

            egui::Window::new("Keyboard Shortcuts")
                .open(&mut is_open)
                .collapsible(false)
                .resizable(true)
                .default_width(1100.0)
                .min_width(400.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    // Read width from the Window body BEFORE the ScrollArea.
                    // Thresholds are deliberately low so that a display with
                    // 150% scaling (which halves reported width in logical
                    // points) still gets 2+ columns.
                    let avail = ui.available_width();
                    let num_cols = if avail >= 850.0 {
                        3
                    } else if avail >= 500.0 {
                        2
                    } else {
                        1
                    };
                    let per_col: &[usize] = match num_cols {
                        3 => &[2, 2, 2],
                        2 => &[3, 3],
                        _ => &[6],
                    };

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .max_height(800.0)
                        .show(ui, |ui| {
                            ui.columns(num_cols, |cols| {
                                let mut idx = 0usize;
                                for (ci, &count) in per_col.iter().enumerate() {
                                    let col_ui = &mut cols[ci];
                                    for si in 0..count {
                                        if idx >= sections.len() {
                                            break;
                                        }
                                        let sec = &sections[idx];
                                        // Blank line before every heading except
                                        // the first one at the top of a column.
                                        if si > 0 {
                                            col_ui.add_space(12.0);
                                        }
                                        egui::Grid::new(("sc_grid", ci, si))
                                            .num_columns(2)
                                            .spacing([20.0, 4.0])
                                            .striped(true)
                                            .show(col_ui, |ui| {
                                                ui.heading(sec.title);
                                                ui.label("");
                                                ui.end_row();
                                                for (label, keys) in sec.rows {
                                                    ui.label(*label);
                                                    ui.label(
                                                        egui::RichText::new(*keys)
                                                            .color(dim_color),
                                                    );
                                                    ui.end_row();
                                                }
                                            });
                                        idx += 1;
                                    }
                                }
                            });
                        });

                    ui.horizontal(|ui| {
                        if ui.button("Close").clicked() {
                            close_clicked = true;
                        }
                    });
                });

            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) || close_clicked || !is_open {
                self.show_shortcuts = false;
            }
        }

        // ---- Keyboard handlers ----
        if !ctx.wants_keyboard_input() && !self.modal_open() {
            // File Ops
            if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.rendered_entries().get(idx) {
                        self.active_dialog = Some(("Rename".to_string(), item.name.clone()));
                    }
                }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::N)) {
                self.active_dialog = Some(("NewFolder".to_string(), String::new()));
            } else if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::N)) {
                self.active_dialog = Some(("NewFile".to_string(), String::new()));
            }
            if ctx.input(|i| {
                i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Delete)
            }) {
                self.confirm_empty_bin = true;
            }
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::C)) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.rendered_entries().get(idx) {
                        ctx.copy_text(item.path.to_string_lossy().to_string());
                        self.status_msg = "Path copied to clipboard".to_string();
                    }
                }
            }

            // View & System
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::H)) {
                self.config.show_hidden = !self.config.show_hidden;
                self.save_cfg();
                self.refresh_keep_selection();
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Num1)) {
                self.entries.sort_by(|a, b| {
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Num2)) {
                self.entries.sort_by(|a, b| {
                    b.is_dir.cmp(&a.is_dir).then_with(|| b.size.cmp(&a.size))
                });
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Num3)) {
                self.entries.sort_by(|a, b| {
                    b.is_dir.cmp(&a.is_dir).then_with(|| {
                        b.mtime
                            .partial_cmp(&a.mtime)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                });
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::R)) {
                self.refresh_keep_selection();
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Q)) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            // Quick access
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::P)) {
                if !self.is_virtual_view() {
                    let p = self.cwd.clone();
                    if !self.config.pins.contains(&p) {
                        self.config.pins.push(p);
                        self.save_cfg();
                        self.status_msg = "Pinned to Quick Access".to_string();
                    } else {
                        self.config.pins.retain(|x| x != &p);
                        self.save_cfg();
                        self.status_msg = "Unpinned from Quick Access".to_string();
                    }
                }
            }

            // Go to tab 1-9
            for i in 1..=9 {
                let key = match i {
                    1 => egui::Key::Num1,
                    2 => egui::Key::Num2,
                    3 => egui::Key::Num3,
                    4 => egui::Key::Num4,
                    5 => egui::Key::Num5,
                    6 => egui::Key::Num6,
                    7 => egui::Key::Num7,
                    8 => egui::Key::Num8,
                    _ => egui::Key::Num9,
                };
                if ctx.input(|inp| inp.modifiers.alt && inp.key_pressed(key)) {
                    if self.tabs.len() >= i {
                        self.load_tab(i - 1);
                    }
                }
            }

            // Next / Prev tab
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::PageDown)) {
                if self.tabs.len() > 1 {
                    let next = (self.cur_tab + 1) % self.tabs.len();
                    self.load_tab(next);
                }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::PageUp)) {
                if self.tabs.len() > 1 {
                    let prev = if self.cur_tab == 0 {
                        self.tabs.len() - 1
                    } else {
                        self.cur_tab - 1
                    };
                    self.load_tab(prev);
                }
            }

            if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    self.selected_item = Some(vis[0]);
                    self.selected_items.clear();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::End)) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    self.selected_item = Some(*vis.last().unwrap());
                    self.selected_items.clear();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::PageUp)) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self
                        .selected_item
                        .and_then(|s| vis.iter().position(|&v| v == s))
                        .unwrap_or(0);
                    let new_pos = pos.saturating_sub(15);
                    self.selected_item = Some(vis[new_pos]);
                    self.selected_items.clear();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::PageDown)) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self
                        .selected_item
                        .and_then(|s| vis.iter().position(|&v| v == s))
                        .unwrap_or(0);
                    let new_pos = (pos + 15).min(vis.len() - 1);
                    self.selected_item = Some(vis[new_pos]);
                    self.selected_items.clear();
                }
            }
            if ctx.input(|i| {
                i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::M)
            }) {
                navigate_to = Some(THIS_PC.to_string());
            }
            if ctx.input(|i| {
                i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::B)
            }) {
                navigate_to = Some(RECYCLE.to_string());
            }
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::Home)) {
                if let Some(ud) = directories::UserDirs::new() {
                    navigate_to = Some(ud.home_dir().to_string_lossy().to_string());
                }
            }

            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                if let Some(idx) = self.selected_item {
                    if self.selected_items.contains(&idx) {
                        self.selected_items.retain(|&x| x != idx);
                    } else {
                        self.selected_items.push(idx);
                    }
                }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::A)) {
                self.selected_items = self.visible_indices();
            }
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::ArrowDown)) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self
                        .selected_item
                        .and_then(|s| vis.iter().position(|&v| v == s))
                        .unwrap_or(0);
                    let new_pos = (pos + 1).min(vis.len() - 1);
                    self.selected_item = Some(vis[new_pos]);
                    if !self.selected_items.contains(&vis[new_pos]) {
                        self.selected_items.push(vis[new_pos]);
                    }
                }
            }
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::ArrowUp)) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self
                        .selected_item
                        .and_then(|s| vis.iter().position(|&v| v == s))
                        .unwrap_or(0);
                    let new_pos = pos.saturating_sub(1);
                    self.selected_item = Some(vis[new_pos]);
                    if !self.selected_items.contains(&vis[new_pos]) {
                        self.selected_items.push(vis[new_pos]);
                    }
                }
            }

            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self
                        .selected_item
                        .and_then(|s| vis.iter().position(|&v| v == s));
                    self.selected_item = Some(match pos {
                        Some(p) => vis[(p + 1).min(vis.len() - 1)],
                        None => vis[0],
                    });
                    self.selected_items.clear();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                let vis = self.visible_indices();
                if !vis.is_empty() {
                    let pos = self
                        .selected_item
                        .and_then(|s| vis.iter().position(|&v| v == s));
                    self.selected_item = Some(match pos {
                        Some(p) => vis[p.saturating_sub(1)],
                        None => vis[0],
                    });
                    self.selected_items.clear();
                }
            }

            // **NEW**: plain Left arrow -> go back
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft) && !i.modifiers.any()) {
                nav_back = true;
            }
            // **NEW**: plain Right arrow -> enter folder, or open file
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight) && !i.modifiers.any()) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.rendered_entries().get(idx) {
                        if item.is_dir {
                            navigate_to = Some(item.path.to_string_lossy().to_string());
                        } else {
                            pending_action = UiAction::Open(item.path.clone());
                        }
                    }
                }
            }

            // Enter: enter folder (old behavior) or open file (new: fallback
            // to Open With on failure)
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.rendered_entries().get(idx) {
                        if item.is_dir {
                            navigate_to = Some(item.path.to_string_lossy().to_string());
                        } else {
                            pending_action = UiAction::Open(item.path.clone());
                        }
                    }
                }
            }

            // Ctrl+Shift+O: Open With… for the selected file
            if ctx.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::O)) {
                if let Some(idx) = self.selected_item {
                    if let Some(item) = self.rendered_entries().get(idx) {
                        if !item.is_dir {
                            pending_action = UiAction::OpenWith(item.path.clone());
                        }
                    }
                }
            }

            if ctx.input(|i| i.key_pressed(egui::Key::Backspace) && !i.modifiers.shift) {
                let has_history =
                    self.tabs.get(self.cur_tab).map_or(false, |t| !t.back.is_empty());
                if has_history {
                    nav_back = true;
                } else {
                    navigate_to = self.parent_location();
                }
            }
            if ctx.input(|i| {
                (i.key_pressed(egui::Key::Backspace) && i.modifiers.shift)
                    || (i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp))
            }) {
                navigate_to = self.parent_location();
            }
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) {
                nav_back = true;
            }
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) {
                nav_forward = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
                if self.search_active() {
                    self.last_search_query.clear();
                } else {
                    self.refresh_keep_selection();
                }
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
            if ctx.input(|i| {
                i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::D)
            }) {
                self.config.dark = !self.config.dark;
                self.theme = if self.config.dark { DARK } else { LIGHT };
                self.save_cfg();
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::T)) {
                self.new_tab();
            }
            if ctx.input(|i| {
                i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Tab)
            }) {
                if self.tabs.len() > 1 {
                    let prev = if self.cur_tab == 0 {
                        self.tabs.len() - 1
                    } else {
                        self.cur_tab - 1
                    };
                    self.load_tab(prev);
                }
            } else if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Tab)) {
                if self.tabs.len() > 1 {
                    let next = (self.cur_tab + 1) % self.tabs.len();
                    self.load_tab(next);
                }
            }
            if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::W)) {
                if self.tabs.len() > 1 {
                    self.close_tab();
                }
            }
        }

        // ---- Panel 4: status bar ----
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.search_active() && self.search_running {
                    ui.label(format!("Searching... ({} found)", self.search_results.len()));
                } else if self.search_active() && self.search_truncated {
                    ui.label(format!(
                        "{}+ found for \"{}\" (capped)",
                        self.search_results.len(),
                        self.search_query
                    ));
                } else if self.search_active() {
                    ui.label(format!(
                        "{} found for \"{}\"",
                        self.search_results.len(),
                        self.search_query
                    ));
                } else {
                    let total = self.entries.len();
                    let shown_n = if self.search_query.is_empty() {
                        total
                    } else {
                        self.visible_indices().len()
                    };
                    if shown_n == total {
                        ui.label(format!("{} items", total));
                    } else {
                        ui.label(format!("{} of {} items", shown_n, total));
                    }
                }
                if !self.status_msg.is_empty() {
                    ui.separator();
                    ui.label(&self.status_msg);
                }
            });
        });

        // ---- Panel 5: file list ----
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.style_mut().interaction.selectable_labels = false;
        let pointer_pos = ui.input(|i| i.pointer.interact_pos());
            let primary_down = ui.input(|i| i.pointer.primary_down());
            if ui.input(|i| i.pointer.primary_pressed()) {
                let on_panel = pointer_pos
                    .map(|p| {
                        ui.max_rect().contains(p) && ctx.layer_id_at(p) == Some(ui.layer_id())
                    })
                    .unwrap_or(false);
                if on_panel {
                    self.lasso_start = pointer_pos;
                    self.selected_items.clear();
                } else {
                    self.lasso_start = None;
                }
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
            if lasso_rect.is_some() {
                self.selected_items.clear();
                self.selected_item = None;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    let filter = self.search_query.to_lowercase();
                    let is_pc = self.cwd == THIS_PC;
                    let in_bin = self.cwd == RECYCLE;
                    let ppp = ctx.pixels_per_point();
                    let row_h = if is_pc { 36.0 } else { 24.0 };
                    let size_title = if is_pc {
                        pc_title_for("size").unwrap_or("")
                    } else {
                        title_for("size").unwrap_or("Size")
                    };
                    let mod_title = if is_pc {
                        pc_title_for("modified").unwrap_or("")
                    } else if in_bin {
                        bin_title_for("modified").unwrap_or("Date deleted")
                    } else {
                        title_for("modified").unwrap_or("Modified")
                    };
                    let searching = self.search_active();
                    let shown: &[Item] = if searching {
                        &self.search_results
                    } else {
                        &self.entries
                    };
                    let size_w = self.config.size_col_width.clamp(60.0, 600.0);
                    let mod_w = self.config.mod_col_width.clamp(60.0, 400.0);
                    let loc_w = 240.0_f32;
                    let mut size_drag = 0.0_f32;
                    let mut mod_drag = 0.0_f32;
                    let mut drag_stopped = false;
                    let row_right = ui.max_rect().right();
                    let mut table = egui_extras::TableBuilder::new(ui)
                        .vscroll(false)
                        .column(egui_extras::Column::remainder().clip(true));
                    if searching {
                        table = table.column(egui_extras::Column::exact(loc_w));
                    }
                    table
                        .column(egui_extras::Column::exact(size_w))
                        .column(egui_extras::Column::exact(mod_w))
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.strong("Name");
                            });
                            if searching {
                                header.col(|ui| {
                                    ui.strong(title_for("where").unwrap_or("Location"));
                                });
                            }
                            header.col(|ui| {
                                ui.strong(size_title);
                                let r = ui.max_rect();
                                let handle = egui::Rect::from_x_y_ranges(
                                    (r.left() - 4.0)..=(r.left() + 4.0),
                                    r.top()..=r.bottom(),
                                );
                                let resp = ui.interact(
                                    handle,
                                    ui.id().with("resize_size_col"),
                                    egui::Sense::drag(),
                                );
                                if resp.hovered() {
                                    ui.ctx()
                                        .set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                                }
                                if resp.dragged() {
                                    size_drag -= resp.drag_delta().x;
                                }
                                if resp.drag_stopped() {
                                    drag_stopped = true;
                                }
                            });
                            header.col(|ui| {
                                ui.strong(mod_title);
                                let r = ui.max_rect();
                                let handle = egui::Rect::from_x_y_ranges(
                                    (r.left() - 4.0)..=(r.left() + 4.0),
                                    r.top()..=r.bottom(),
                                );
                                let resp = ui.interact(
                                    handle,
                                    ui.id().with("resize_mod_col"),
                                    egui::Sense::drag(),
                                );
                                if resp.hovered() {
                                    ui.ctx()
                                        .set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                                }
                                if resp.dragged() {
                                    mod_drag -= resp.drag_delta().x;
                                }
                                if resp.drag_stopped() {
                                    drag_stopped = true;
                                }
                            });
                        })
                        .body(|mut body| {
                            for (idx, item) in shown.iter().enumerate() {
                                if !filter.is_empty()
                                    && !item.name.to_lowercase().contains(&filter)
                                {
                                    continue;
                                }
                                let name_str = item.name.clone();
                                let size_str = if is_pc {
                                    human_size(item.size)
                                } else if item.is_dir {
                                    String::new()
                                } else {
                                    human_size(item.size)
                                };
                                let time_str = fmt_time(item.mtime);
                                let is_dir = item.is_dir;
                                let item_path = item.path.clone();
                                let item_name = item.name.clone();
                                let item_orig = item.orig.clone();
                                let item_total = item.total;
                                let item_size = item.size;
                                let icon_name = item_orig
                                    .as_ref()
                                    .and_then(|o| o.file_name())
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| item_name.clone());
                                let icon_kind = if is_pc {
                                    IconKind::Drive(&item_path)
                                } else if is_dir {
                                    IconKind::Folder
                                } else {
                                    IconKind::File {
                                        name: &icon_name,
                                        path: &item_path,
                                    }
                                };
                                let pin_str = item.path.to_string_lossy().to_string();
                                let is_pinned =
                                    self.config.pins.iter().any(|p| p == &pin_str);

                                    body.row(row_h, |mut row| {
                                    row.col(|ui| {
                                        ui.spacing_mut().interact_size.y = row_h;
                                        // Make the row's hit/paint area span every column.
                                        let cell_rect = ui.max_rect();
                                        let row_rect = egui::Rect::from_min_max(
                                            cell_rect.min,
                                            egui::pos2(row_right, cell_rect.max.y),
                                        );
                                        let cell_clip = ui.clip_rect();
                                        ui.set_clip_rect(egui::Rect::from_min_max(
                                            cell_clip.min,
                                            egui::pos2(row_right, cell_clip.max.y),
                                        ));
                                        let response = ui.interact(
                                            row_rect,
                                            ui.id().with(("row_sel", idx)),
                                            egui::Sense::click(),
                                        );
                                        if let Some(lr) = lasso_rect {
                                            if lr.intersects(response.rect) {
                                                if !self.selected_items.contains(&idx) {
                                                    self.selected_items.push(idx);
                                                }
                                                if self.selected_item.is_none() {
                                                    self.selected_item = Some(idx);
                                                }
                                            }
                                        }
                                        ui.spacing_mut().interact_size.y = row_h;
                                        if self.selected_item == Some(idx)
                                            || self.selected_items.contains(&idx)
                                        {
                                            ui.painter().rect_filled(
                                                response.rect,
                                                0.0,
                                                sel_fill(&self.theme, ui),
                                            );
                                        } else if response.hovered() {
                                            ui.painter().rect_filled(
                                                response.rect,
                                                0.0,
                                                self.theme.hover,
                                            );
                                        }
                                        if response.clicked() {
                                            self.selected_item = Some(idx);
                                        }
                                        if response.secondary_clicked() {
                                            if !self.selected_items.contains(&idx) {
                                                self.selected_items.clear();
                                            }
                                            self.selected_item = Some(idx);
                                        }
                                        if response.double_clicked() {
                                            if is_dir {
                                                navigate_to = Some(
                                                    item_path.to_string_lossy().to_string(),
                                                );
                                            } else {
                                                pending_action =
                                                    UiAction::Open(item_path.clone());
                                            }
                                        }

                                        ui.set_clip_rect(cell_clip);
                                        let visible = ui.is_rect_visible(ui.max_rect());
                                        ui.horizontal(|ui| {
                                            let tex = if visible {
                                                self.icons.get(ctx, icon_kind)
                                            } else {
                                                None
                                            };
                                            draw_icon(ui, ppp, tex.as_ref(), is_dir);
                                            ui.label(name_str);
                                            if let Some(o) = &item_orig {
                                                ui.weak(format!("  (from {})", o.display()));
                                            }
                                        });
                                        
                                        response.context_menu(|ui| {
                                            if ui.button("Open").clicked() {
                                                pending_action =
                                                    UiAction::Open(item_path.clone());
                                                ui.close_menu();
                                            }
                                            if !is_dir && ui.button("Open With…").clicked() {
                                                pending_action =
                                                    UiAction::OpenWith(item_path.clone());
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
                                            if is_dir && !in_bin {
                                                ui.separator();
                                                let label = if is_pinned {
                                                    "Unpin from Quick Access"
                                                } else {
                                                    "Pin to Quick Access"
                                                };
                                                if ui.button(label).clicked() {
                                                    pending_action =
                                                        UiAction::TogglePin(item_path.clone());
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                    });

                                    if searching {
                                        row.col(|ui| {
                                            if self.selected_item == Some(idx)
                                                || self.selected_items.contains(&idx)
                                            {
                                                ui.painter().rect_filled(
                                                    ui.max_rect(),
                                                    0.0,
                                                    sel_fill(&self.theme, ui),
                                                );
                                            }
                                            let loc = item_path
                                                .parent()
                                                .map(|p| p.to_string_lossy().to_string())
                                                .unwrap_or_default();
                                            let loc = if loc.chars().count() > 60 {
                                                format!(
                                                    "{}...",
                                                    loc.chars().take(57).collect::<String>()
                                                )
                                            } else {
                                                loc
                                            };
                                            ui.label(
                                                egui::RichText::new(loc).color(self.theme.dim),
                                            );
                                        });
                                    }
row.col(|ui| {
    ui.spacing_mut().interact_size.y = row_h;
    if self.selected_item == Some(idx)
        || self.selected_items.contains(&idx)
    {
        ui.painter().rect_filled(ui.max_rect(), 0.0, sel_fill(&self.theme, ui));
    }
    if is_pc && item_total > 0 {
        let used = item_total.saturating_sub(item_size);
        let frac = (used as f32 / item_total as f32).clamp(0.0, 1.0);
        let fill = if frac > 0.9 {
            self.theme.danger
        } else {
            self.theme.accent
        };
        let track = self.theme.border;
        ui.vertical(|ui| {
            let w = (ui.available_width() - 12.0).max(60.0);
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(w, 8.0),
                egui::Sense::hover(),
            );
            ui.painter().rect_filled(rect, 2.0, track);
            let filled = egui::Rect::from_min_size(
                rect.min,
                egui::vec2(rect.width() * frac, rect.height()),
            );
            ui.painter().rect_filled(filled, 2.0, fill);
            ui.label(format!(
                "{} free of {} ({:.0}% used)",
                human_size(item_size),
                human_size(item_total),
                frac * 100.0
            ));
        });
    } else {
        ui.horizontal(|ui| {
            ui.label(size_str);
        });
    }
});
row.col(|ui| {
    let r = ui.max_rect();
    if self.selected_item == Some(idx)
        || self.selected_items.contains(&idx)
    {
        ui.painter().rect_filled(r, 0.0, sel_fill(&self.theme, ui));
    }
    ui.painter().text(
        egui::pos2(r.left() + 4.0, r.center().y),
        egui::Align2::LEFT_CENTER,
        &time_str,
        egui::TextStyle::Body.resolve(ui.style()),
        self.theme.dim,
    );
});
});
}
});
                    if size_drag != 0.0 || mod_drag != 0.0 {
                        self.config.size_col_width =
                            (self.config.size_col_width + size_drag).clamp(60.0, 600.0);
                        self.config.mod_col_width =
                            (self.config.mod_col_width + mod_drag).clamp(60.0, 400.0);
                    }
                    if drag_stopped {
                        self.save_cfg();
                    }

                    let remaining_height = (ui.clip_rect().bottom() - ui.cursor().top()).max(0.0);
                    if remaining_height > 0.0 {
                        let remaining_rect = egui::Rect::from_min_size(
                            ui.cursor().min,
                            egui::vec2(ui.available_width(), remaining_height),
                        );
                        let response =
                            ui.allocate_rect(remaining_rect, egui::Sense::click_and_drag());
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
                        if response.clicked() {
                            self.selected_item = None;
                            self.selected_items.clear();
                        }
                    }
                });

            if let Some(lr) = lasso_rect {
                stroke_rect(
                    ui.painter(),
                    lr,
                    egui::Stroke::new(1.0_f32, self.theme.hover),
                );
            }
            if ui.input(|i| i.pointer.primary_released()) {
                self.lasso_start = None;
            }
        });

        // ---- Action dispatch ----
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
                self.save_cfg();
            }
            UiAction::Open(path) => {
                if path.is_dir() {
                    navigate_to = Some(path.to_string_lossy().to_string());
                } else {
                    match keyfm::apps::default_open(&path) {
                        Ok(()) => {}
                        Err(_) => {
                            self.open_with = Some(OpenWithState::new(path));
                        }
                    }
                }
            }
            UiAction::OpenWith(path) => {
                if !path.is_dir() {
                    self.open_with = Some(OpenWithState::new(path));
                } else {
                    navigate_to = Some(path.to_string_lossy().to_string());
                }
            }
            UiAction::NewFolder => {
                self.active_dialog = Some(("NewFolder".to_string(), String::new()));
            }
            UiAction::NewFile => {
                self.active_dialog = Some(("NewFile".to_string(), String::new()));
            }
            UiAction::Refresh => {
                if self.search_active() {
                    self.last_search_query.clear();
                } else {
                    self.refresh_keep_selection();
                }
            }
        }

        if self.is_virtual_view() && self.active_dialog.is_some() {
            self.active_dialog = None;
            self.status_msg = "Not available in this view".to_string();
        }

        // ---- Empty Recycle Bin dialog ----
        if self.confirm_empty_bin {
            let (mut do_empty, mut close) = (false, false);
            egui::Window::new("Empty Recycle Bin")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(
                        "Permanently delete everything in the Recycle Bin? This cannot be undone.",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Empty").clicked() {
                            do_empty = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            if do_empty {
                self.status_msg = match keyfm::trash::win_empty_recycle_bin() {
                    Ok(()) => String::new(),
                    Err(e) => format!("Empty failed: {}", e),
                };
                self.refresh_dir();
                close = true;
            }
            if close {
                self.confirm_empty_bin = false;
            }
        }

        // ---- Open With dialog ----
        if self.open_with.is_some() {
            let mut launch_exe: Option<String> = None;
            let mut close_dialog = false;

            if let Some(state) = self.open_with.as_mut() {
                let target_name = state
                    .target
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| state.target.display().to_string());

                let mut start_picker = false;

                egui::Window::new("Open With")
                    .collapsible(false)
                    .resizable(true)
                    .default_width(420.0)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "Choose an application to open \"{}\":",
                            target_name
                        ));
                        ui.add_space(6.0);

                        let apps = keyfm::apps::list_apps();
                        let filter_resp = ui.add(
                            egui::TextEdit::singleline(&mut state.filter)
                                .hint_text("Filter applications… (Enter opens the first match)")
                                .desired_width(f32::INFINITY),
                        );
                        if state.focus_filter {
                            filter_resp.request_focus();
                            state.focus_filter = false;
                        }
                        ui.add_space(4.0);

                        let needle = state.filter.trim().to_lowercase();
                        let matches: Vec<&(String, String)> = apps
                            .iter()
                            .filter(|a| needle.is_empty() || a.0.to_lowercase().contains(&needle))
                            .collect();

                        if filter_resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
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
                            egui::ScrollArea::vertical()
                                .max_height(300.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
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
                            if ui.button("Choose another program…").clicked() {
                                start_picker = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close_dialog = true;
                            }
                        });
                    });

                if start_picker {
                    state.start_picker();
                }

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
                            program_picker_ui(
                                ui, vctx, state, icons, &theme, known, drives_c, &target_name,
                            )
                        };
                        if class == egui::ViewportClass::Embedded {
                            egui::Window::new("Open with…")
                                .default_size([780.0, 500.0])
                                .show(vctx, |ui| {
                                    outcome = body(ui);
                                });
                        } else {
                            egui::CentralPanel::default().show(vctx, |ui| {
                                outcome = body(ui);
                            });
                        }
                        if vctx.input(|i| i.viewport().close_requested()) {
                            outcome.close = true;
                        }
                    });

                    if let Some(p) = outcome.choose {
                        launch_exe = Some(p.to_string_lossy().to_string());
                        close_dialog = true;
                    } else if outcome.close {
                        state.picker_open = false;
                    }
                }

                // Escape: picker first, then dialog
                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    if state.picker_open {
                        state.picker_open = false;
                    } else {
                        close_dialog = true;
                    }
                }
            }

            if let Some(exe) = launch_exe {
                let target = self.open_with.as_ref().map(|s| s.target.clone());
                self.open_with = None;
                if let Some(target) = target {
                    match keyfm::apps::launch_with(&exe, &target) {
                        Ok(()) => {
                            let app = Path::new(&exe)
                                .file_stem()
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

        // ---- Rename / New File / New Folder dialog ----
        if let Some((action, mut input_val)) = self.active_dialog.take() {
            let fresh = !self.dialog_seen;
            self.dialog_seen = true;
            let mut confirm = false;
            let mut cancel = false;

            egui::Window::new(action.as_str())
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Enter name for {}:", action));
                    let resp = ui.text_edit_singleline(&mut input_val);
                    if fresh {
                        resp.request_focus();
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        confirm = true;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        cancel = true;
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
                let target = base.join(&name);

                let result = (|| -> Result<(), String> {
                    keyfm::helpers::validate_name(&name)?;
                    match action.as_str() {
                        "NewFolder" => {
                            if target.symlink_metadata().is_ok() {
                                return Err(format!("\"{}\" already exists", name));
                            }
                            std::fs::create_dir(&target)
                                .map_err(|e| format!("Create folder failed: {}", e))
                        }
                        "NewFile" => std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&target)
                            .map(|_| ())
                            .map_err(|e| {
                                if e.kind() == std::io::ErrorKind::AlreadyExists {
                                    format!("\"{}\" already exists", name)
                                } else {
                                    format!("Create file failed: {}", e)
                                }
                            }),
                        "Rename" => {
                            let idx = self.selected_item.ok_or("No item selected")?;
                            let old = self
                                .rendered_entries()
                                .get(idx)
                                .ok_or("No item selected")?
                                .path
                                .clone();
                            if old == target {
                                return Ok(());
                            }
                            let same_item = keyfm::helpers::norm(&old.to_string_lossy())
                                == keyfm::helpers::norm(&target.to_string_lossy());
                            if target.symlink_metadata().is_ok() && !same_item {
                                return Err(format!("\"{}\" already exists", name));
                            }
                            std::fs::rename(&old, &target)
                                .map_err(|e| format!("Rename failed: {}", e))
                        }
                        _ => Ok(()),
                    }
                })();

                match result {
                    Ok(()) => {
                        self.status_msg.clear();
                        self.refresh_dir();
                        if let Some(pos) = self
                            .rendered_entries()
                            .iter()
                            .position(|it| it.path == target)
                        {
                            self.selected_item = Some(pos);
                        }
                    }
                    Err(msg) => {
                        self.status_msg = msg;
                        self.active_dialog = Some((action, input_val));
                    }
                }
            } else if !cancel {
                self.active_dialog = Some((action, input_val));
            }

            if self.active_dialog.is_none() {
                self.dialog_seen = false;
            }
        }

        // ---- Navigation dispatch ----
        if nav_back {
            self.go_back();
        } else if nav_forward {
            self.go_forward();
        } else if let Some(raw) = navigate_to {
            match resolve_location(&raw, &self.cwd) {
                Some(loc) => {
                    self.go_to(loc);
                    self.selected_item = self.visible_indices().first().copied();
                }
                None => {
                    self.status_msg = format!("Not a directory: {}", raw);
                    self.path_input = self.cwd_display();
                }
            }
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let args: Vec<String> = env::args().collect();
    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" {
            println!("keyfm {} - Keyboard-driven desktop file manager", keyfm::VERSION);
            println!("Usage: keyfm [START_DIR]");
            return Ok(());
        }
        if arg == "-V" || arg == "--version" {
            println!("keyfm {}", keyfm::VERSION);
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
        Box::new(|cc| {
            // --- LOAD NATIVE WINDOWS FONT ---
            let mut fonts = egui::FontDefinitions::default();
            
            #[cfg(windows)]
            {
                // Segoe UI is the standard Windows 10/11 font.
                if let Ok(font_data) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
                    fonts.font_data.insert("segoe".to_owned(), egui::FontData::from_owned(font_data));
                    
                    // Set it as the highest priority font for normal text
                    fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "segoe".to_owned());
                    // Also use it for monospace (like paths) so it looks cohesive
                    fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().insert(0, "segoe".to_owned());
                }
            }
            
            cc.egui_ctx.set_fonts(fonts);
            // --------------------------------

            Ok(Box::new(KeyFmApp::new(start)))
        }),
    )
}

const SEARCH_DEBOUNCE_MS: u64 = 150;
const SEARCH_RESULT_CAP: usize = 5000;
const SKIP_DIRS: &[&str] = &[
    "$Recycle.Bin",
    "System Volume Information",
    "Windows",
    "node_modules",
    ".git",
];

pub struct SearchHit {
    pub path: std::path::PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: Option<f64>,
}

impl SearchHit {
    fn into_item(self) -> Item {
        Item {
            name: self.name,
            path: self.path,
            is_dir: self.is_dir,
            size: self.size,
            mtime: self.mtime,
            hidden: false,
            extra: String::new(),
            orig: None,
            info: String::new(),
            total: 0,
        }
    }
}

fn run_walk(
    root: std::path::PathBuf,
    needle: String,
    show_hidden: bool,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tx: std::sync::mpsc::Sender<SearchHit>,
) {
    use std::sync::atomic::Ordering;
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };

            let name_os = entry.file_name();
            let name = name_os.to_string_lossy();
            let is_dir = meta.is_dir();
            let hidden = name.starts_with('.') || keyfm::helpers::is_windows_hidden(Some(&meta));

            if is_dir && !hidden {
                let skip = SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s));
                if !skip {
                    stack.push(path.clone());
                }
            }

            if !show_hidden && hidden {
                continue;
            }

            if name.to_lowercase().contains(&needle) {
                let size = if is_dir { 0 } else { meta.len() };
                let mtime = meta.modified().ok().and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs_f64())
                });
                if tx
                    .send(SearchHit {
                        path,
                        name: name.into_owned(),
                        is_dir,
                        size,
                        mtime,
                    })
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}
