use directories::UserDirs;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Local};
use crate::constants::{THIS_PC, RECYCLE};

pub fn human_size(n: u64) -> String {
    if n < 1024 {
        return format!("{} B", n);
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut val = n as f64;
    let mut idx = 0;
    while val >= 1024.0 && idx < units.len() - 1 {
        val /= 1024.0;
        idx += 1;
    }
    format!("{:.1} {}", val, units[idx])
}

pub fn fmt_time(ts: Option<f64>) -> String {
    let Some(t) = ts else { return String::new() };
    if t.is_nan() || t.is_infinite() {
        return String::new();
    }
    let secs = t.trunc() as i64;
    let nsecs = ((t.fract() * 1e9).abs()) as u32;
    let dt = DateTime::<Local>::from_timestamp(secs, nsecs);
    match dt {
        Some(d) => d.format("%Y-%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

pub fn norm(p: &str) -> String {
    let path = Path::new(p);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
    };
    
    let mut components = Vec::new();
    for c in abs.components() {
        match c {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            std::path::Component::Normal(s) => {
                components.push(s);
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                components.push(c.as_os_str());
            }
        }
    }
    let built: PathBuf = components.iter().collect();
    let mut s = built.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        s = s.to_lowercase().replace('/', "\\");
    }
    s
}

pub fn same_loc(a: &str, b: &str) -> bool {
    if a == THIS_PC || a == RECYCLE || b == THIS_PC || b == RECYCLE {
        return a == b;
    }
    norm(a) == norm(b)
}

pub fn unique_path(folder: &Path, name: &str, is_dir: bool) -> PathBuf {
    let target = folder.join(name);
    if !target.symlink_metadata().is_ok() {
        return target;
    }
    
    let (stem, ext) = if is_dir {
        (name.to_string(), String::new())
    } else {
        let p = Path::new(name);
        let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string());
        (stem, ext)
    };

    let mut i = 1;
    loop {
        let new_name = if i == 1 {
            format!("{} - copy{}", stem, ext)
        } else {
            format!("{} - copy {}{}", stem, i, ext)
        };
        let candidate = folder.join(new_name);
        if !candidate.symlink_metadata().is_ok() {
            return candidate;
        }
        i += 1;
    }
}

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
