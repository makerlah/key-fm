use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct ClipItem {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
}

pub(crate) enum UiAction {
    None,
    Copy,
    Cut,
    Paste,
    Rename(String),
    Delete,
    TogglePin(PathBuf),
    Open(PathBuf),
    OpenWith(PathBuf),
    SidebarPin(PathBuf),
    SidebarUnpin(PathBuf),
    NewFolder,
    NewFile,
    Refresh,
}