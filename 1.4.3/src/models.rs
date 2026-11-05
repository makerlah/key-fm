use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: Option<f64>,
    pub hidden: bool,
    pub extra: String,
    pub orig: Option<PathBuf>,
    pub info: String,
    /// Total capacity in bytes (drives in the This PC view; 0 = unknown).
    pub total: u64,
}

#[derive(Clone, Debug)]
pub struct Tab {
    pub cwd: Option<PathBuf>,
    pub back: Vec<PathBuf>,
    pub fwd: Vec<PathBuf>,
    pub sort_col: String,
    pub sort_reverse: bool,
    pub search_query: String,
    pub sel: Vec<PathBuf>,
    pub focus: Option<PathBuf>,
}

impl Tab {
    pub fn new(cwd: Option<PathBuf>) -> Self {
        Self {
            cwd,
            back: Vec::new(),
            fwd: Vec::new(),
            sort_col: "name".to_string(),
            sort_reverse: false,
            search_query: String::new(),
            sel: Vec::new(),
            focus: None,
        }
    }
}
