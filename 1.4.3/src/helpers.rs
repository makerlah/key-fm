use directories::UserDirs;
use std::io;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Local, Utc};
use crate::constants::{THIS_PC, RECYCLE, QUICK_ACCESS};

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
    let secs = t.floor() as i64;
    let nsecs = (((t - t.floor()) * 1e9) as u32).min(999_999_999);
    let dt = DateTime::<Utc>::from_timestamp(secs, nsecs).map(|d| d.with_timezone(&Local));
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
    if a == THIS_PC || a == RECYCLE || a == QUICK_ACCESS
        || b == THIS_PC || b == RECYCLE || b == QUICK_ACCESS
    {
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
    // Only a bare "~" or a leading "~/" / "~\\" is a home reference ("~foo" is not).
    if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        if let Some(ud) = UserDirs::new() {
            return format!("{}{}", ud.home_dir().to_string_lossy(), &path[1..]);
        }
    }
    path.to_string()
}

/// Lexically resolves `.` and `..` (no filesystem access, no case changes) so that
/// "Up" from a typed path like "/a/b/.." behaves sensibly.
pub fn clean_path(p: &Path) -> PathBuf {
    let mut parts: Vec<std::path::Component> = Vec::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if matches!(parts.last(), Some(std::path::Component::Normal(_))) {
                    parts.pop();
                }
            }
            other => parts.push(other),
        }
    }
    parts.iter().map(|c| c.as_os_str()).collect()
}

/// Expands a leading `~`, makes the path absolute, cleans `.`/`..`, and returns it
/// only if it is an existing directory.
pub fn expand_and_validate(raw: &str) -> Option<PathBuf> {
    let base = std::env::current_dir().ok()?;
    expand_and_validate_in(raw, &base)
}

/// Like [`expand_and_validate`], but relative paths resolve against `base`
/// (the folder being browsed) instead of the process working directory.
pub fn expand_and_validate_in(raw: &str, base: &Path) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded = shellexpand_tilde(trimmed);
    let p = Path::new(&expanded);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    };
    let cleaned = clean_path(&abs);
    if cleaned.as_os_str().is_empty() || !cleaned.is_dir() {
        return None;
    }
    Some(cleaned)
}

pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." {
        return Err("Invalid name".into());
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err("Name cannot contain / or \\".into());
    }
    #[cfg(windows)]
    if name.chars().any(|c| matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return Err("Name contains characters Windows does not allow".into());
    }
    Ok(())
}

/// True for entries with the Windows FILE_ATTRIBUTE_HIDDEN bit set (always false elsewhere).
pub fn is_windows_hidden(meta: Option<&std::fs::Metadata>) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.map_or(false, |m| m.file_attributes() & 0x2 != 0)
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}

/// Deletes a file, symlink or directory tree without following symlinks.
pub fn delete_permanently(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else if meta.file_type().is_symlink() {
        // Windows directory symlinks need remove_dir.
        std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path))
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(unix)]
fn copy_symlink(from: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(from)?, to)
}

#[cfg(windows)]
fn copy_symlink(from: &Path, to: &Path) -> io::Result<()> {
    let target = std::fs::read_link(from)?;
    if std::fs::metadata(from).map(|m| m.is_dir()).unwrap_or(false) {
        std::os::windows::fs::symlink_dir(target, to)
    } else {
        std::os::windows::fs::symlink_file(target, to)
    }
}

#[cfg(not(any(unix, windows)))]
fn copy_symlink(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "symlinks are not supported"))
}

/// Recursively copies a directory. Symlinks are recreated, not followed.
/// Refuses to copy a folder into itself.
pub fn copy_dir_all(src: &Path, dst: &Path) -> io::Result<()> {
    if let (Ok(s), Some(Ok(d))) = (src.canonicalize(), dst.parent().map(|p| p.canonicalize())) {
        if d.starts_with(&s) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot copy a folder into itself",
            ));
        }
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            copy_symlink(&from, &to)?;
        } else if ft.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Copies a file, symlink or directory to `dst` (which must not exist).
/// A partially written destination is removed if the copy fails.
pub fn copy_any(src: &Path, dst: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    let res = if meta.file_type().is_symlink() {
        copy_symlink(src, dst)
    } else if meta.is_dir() {
        copy_dir_all(src, dst)
    } else {
        std::fs::copy(src, dst).map(|_| ())
    };
    if res.is_err() && dst.symlink_metadata().is_ok() {
        let _ = delete_permanently(dst);
    }
    res
}

fn is_cross_device(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(18) // EXDEV
    }
    #[cfg(windows)]
    {
        e.raw_os_error() == Some(17) // ERROR_NOT_SAME_DEVICE
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = e;
        false
    }
}

/// Moves `src` to `dst` (which must not exist). Falls back to copy + delete
/// when the two paths are on different filesystems. The source is only removed
/// after the copy fully succeeded.
pub fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if is_cross_device(&e) => {
            copy_any(src, dst)?;
            if let Err(del) = delete_permanently(src) {
                // Do not leave two copies behind with an error that hides it.
                return Err(io::Error::new(
                    del.kind(),
                    format!("copied, but could not remove the original: {}", del),
                ));
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}