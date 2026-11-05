use std::path::{Path, PathBuf};

pub(crate) struct PickerEntry {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) is_dir: bool,
    pub(crate) size: u64,
    pub(crate) mtime: Option<f64>,
}

pub(crate) struct PickerOutcome {
    pub(crate) choose: Option<PathBuf>,
    pub(crate) close: bool,
}

pub(crate) struct OpenWithState {
    pub(crate) target: PathBuf,
    pub(crate) filter: String,
    pub(crate) focus_filter: bool,
    pub(crate) picker_open: bool,
    pub(crate) picker_cwd: PathBuf,
    pub(crate) picker_entries: Vec<PickerEntry>,
    pub(crate) picker_selected: Option<usize>,
    pub(crate) picker_filter: String,
    pub(crate) picker_path_input: String,
    pub(crate) picker_show_all: bool,
    pub(crate) picker_hist: Vec<PathBuf>,
    pub(crate) picker_fwd: Vec<PathBuf>,
    pub(crate) picker_scroll_to: Option<usize>,
    pub(crate) picker_set_offset: Option<f32>,
    pub(crate) picker_focus_filter: bool,
}

impl OpenWithState {
    pub(crate) fn new(target: PathBuf) -> Self {
        Self {
            target, filter: String::new(), focus_filter: true,
            picker_open: false, picker_cwd: PathBuf::new(),
            picker_entries: Vec::new(), picker_selected: None,
            picker_filter: String::new(), picker_path_input: String::new(),
            picker_show_all: false, picker_hist: Vec::new(), picker_fwd: Vec::new(),
            picker_scroll_to: None, picker_set_offset: None, picker_focus_filter: false,
        }
    }

    pub(crate) fn start_picker(&mut self) {
        self.picker_open = true;
        self.picker_hist.clear();
        self.picker_fwd.clear();
        self.picker_filter.clear();
        self.picker_cwd = default_picker_dir();
        self.reload_picker();
    }

    pub(crate) fn reload_picker(&mut self) {
        self.picker_entries.clear();
        self.picker_selected = None;
        self.picker_scroll_to = None;
        self.picker_set_offset = Some(0.0);
        self.picker_path_input = self.picker_cwd.display().to_string();
        let Ok(rd) = std::fs::read_dir(&self.picker_cwd) else { return };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().ok();
            let is_dir = path.is_dir();
            let hidden = (cfg!(unix) && name.starts_with('.'))
                || crate::helpers::is_windows_hidden(meta.as_ref());
            if hidden && !self.picker_show_all { continue; }
            if !is_dir && !self.picker_show_all && !is_executable_candidate(&path) { continue; }
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = meta.as_ref().and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64());
            self.picker_entries.push(PickerEntry { name, path, is_dir, size, mtime });
        }
        self.picker_entries.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
    }

    pub(crate) fn picker_goto(&mut self, p: PathBuf) {
        if p != self.picker_cwd {
            let old = std::mem::replace(&mut self.picker_cwd, p);
            if !old.as_os_str().is_empty() { self.picker_hist.push(old); }
            self.picker_fwd.clear();
        }
        self.picker_filter.clear();
        self.reload_picker();
    }

    pub(crate) fn picker_back(&mut self) {
        if let Some(p) = self.picker_hist.pop() {
            let old = std::mem::replace(&mut self.picker_cwd, p);
            self.picker_fwd.push(old);
            self.picker_filter.clear();
            self.reload_picker();
        }
    }

    pub(crate) fn picker_forward(&mut self) {
        if let Some(p) = self.picker_fwd.pop() {
            let old = std::mem::replace(&mut self.picker_cwd, p);
            self.picker_hist.push(old);
            self.picker_filter.clear();
            self.reload_picker();
        }
    }

    pub(crate) fn picker_up(&mut self) {
        if let Some(parent) = self.picker_cwd.parent().map(|p| p.to_path_buf()) {
            self.picker_goto(parent);
        }
    }
}

pub(crate) fn default_picker_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(p) = std::env::var_os("ProgramFiles") {
            let p = PathBuf::from(p);
            if p.is_dir() { return p; }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let p = PathBuf::from("/Applications");
        if p.is_dir() { return p; }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let p = PathBuf::from("/usr/bin");
        if p.is_dir() { return p; }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub(crate) fn is_executable_candidate(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        matches!(ext.as_str(), "exe" | "bat" | "cmd" | "com")
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
}

pub(crate) const PICKER_ROW_H: f32 = 24.0;

pub(crate) fn picker_shortcuts(known: &[(&'static str, PathBuf)]) -> Vec<(String, PathBuf)> {
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
                if !sub.is_empty() { p.push(sub); }
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