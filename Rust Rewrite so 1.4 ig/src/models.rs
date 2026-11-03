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
}

#[derive(Clone, Debug)]
pub struct Tab {
    pub cwd: Option<PathBuf>,
    pub back: Vec<PathBuf>,
    pub fwd: Vec<PathBuf>,
    pub sort_col: String,
    pub sort_reverse: bool,
    pub filter: String,
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
            filter: String::new(),
            sel: Vec::new(),
            focus: None,
        }
    }
}
