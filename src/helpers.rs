use directories::UserDirs;
use std::path::PathBuf;

pub fn get_start_dir(start: Option<String>) -> PathBuf {
    if let Some(s) = start {
        let expanded = shellexpand_tilde(&s);
        let path = PathBuf::from(expanded);
        if path.is_dir() {
            return path;
        }
    }
    if let Some(ud) = UserDirs::new() {
        return ud.home_dir().to_path_buf();
    }
    PathBuf::from(".")
}

fn shellexpand_tilde(path: &str) -> String {
    if path.starts_with('~') {
        if let Some(ud) = UserDirs::new() {
            return path.replacen('~', ud.home_dir().to_str().unwrap_or(""), 1);
        }
    }
    path.to_string()
}
