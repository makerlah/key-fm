use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use winreg::enums::*;
#[cfg(windows)]
use winreg::RegKey;

static APPS_CACHE: OnceLock<Vec<(String, String)>> = OnceLock::new();

/// (display name, launch command). Scanned once, then served from the cache
/// without cloning (this is called every frame while the dialog is open).
pub fn list_apps() -> &'static [(String, String)] {
    APPS_CACHE.get_or_init(scan_apps).as_slice()
}

fn finish(mut apps: Vec<(String, String)>) -> Vec<(String, String)> {
    apps.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    apps.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
    apps
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let var = &after[..end];
                match std::env::var(var) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(var);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// App Paths values can be `"C:\x\a.exe"`, `C:\x\a.exe`, `"C:\x\a.exe" /flag`
/// or contain %ENV% variables. Returns just the executable path.
#[cfg(windows)]
fn clean_exe_value(raw: &str) -> String {
    let v = expand_env(raw.trim());
    if let Some(stripped) = v.strip_prefix('"') {
        return match stripped.find('"') {
            Some(end) => stripped[..end].to_string(),
            None => stripped.to_string(),
        };
    }
    let lower = v.to_ascii_lowercase();
    match lower.find(".exe") {
        Some(i) => v[..i + 4].to_string(),
        None => v,
    }
}

#[cfg(windows)]
fn scan_apps() -> Vec<(String, String)> {
    let mut apps: Vec<(String, String)> = Vec::new();
    let roots = [
        (HKEY_CURRENT_USER, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths"),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths"),
    ];

    for (hive, path) in &roots {
        let root = RegKey::predef(*hive);
        let Ok(app_paths) = root.open_subkey(path) else {
            continue;
        };

        for exe_name in app_paths.enum_keys().flatten() {
            let Ok(exe_key) = app_paths.open_subkey(&exe_name) else {
                continue;
            };
            let Ok(raw) = exe_key.get_value::<String, _>("") else {
                continue;
            };

            let cleaned = clean_exe_value(&raw);
            // Stale registry entries (uninstalled apps) would just fail to launch.
            if cleaned.is_empty() || !Path::new(&cleaned).is_file() {
                continue;
            }

            let name = match exe_name.len().checked_sub(4) {
                Some(i) if exe_name.is_char_boundary(i) && exe_name[i..].eq_ignore_ascii_case(".exe") => {
                    exe_name[..i].to_string()
                }
                _ => exe_name.clone(),
            };
            apps.push((name, cleaned));
        }
    }
    finish(apps)
}

// ------------------------------------------------------------------ macOS

#[cfg(target_os = "macos")]
fn scan_apps() -> Vec<(String, String)> {
    let mut dirs = vec![std::path::PathBuf::from("/Applications")];
    if let Some(h) = std::env::var_os("HOME") {
        dirs.push(std::path::PathBuf::from(h).join("Applications"));
    }
    let mut apps = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map_or(false, |x| x == "app") {
                if let Some(stem) = p.file_stem() {
                    apps.push((stem.to_string_lossy().to_string(), p.to_string_lossy().to_string()));
                }
            }
        }
    }
    finish(apps)
}

// ------------------------------------------------------------ Linux / unix

#[cfg(all(unix, not(target_os = "macos")))]
fn scan_apps() -> Vec<(String, String)> {
    use std::path::PathBuf;

    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(x).join("applications"));
    } else if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/share/applications"));
    }

    let mut apps = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map_or(true, |x| x != "desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            if let Some(entry) = parse_desktop_entry(&text) {
                apps.push(entry);
            }
        }
    }
    finish(apps)
}

/// Only plain `Exec=program %f` style entries are kept: launching one with extra
/// flags/wrappers (flatpak run ..., env X=1 ...) through a single argv slot would
/// start the wrong thing, so those are skipped instead of listed broken.
#[cfg(all(unix, not(target_os = "macos")))]
fn parse_desktop_entry(text: &str) -> Option<(String, String)> {
    let mut in_entry = false;
    let (mut name, mut exec) = (None::<String>, None::<String>);
    let mut hidden = false;
    let mut is_app = true;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(v) = line.strip_prefix("Name=") {
            name.get_or_insert_with(|| v.to_string());
        } else if let Some(v) = line.strip_prefix("Exec=") {
            exec.get_or_insert_with(|| v.to_string());
        } else if line == "NoDisplay=true" || line == "Hidden=true" {
            hidden = true;
        } else if let Some(v) = line.strip_prefix("Type=") {
            is_app = v == "Application";
        }
    }
    if hidden || !is_app {
        return None;
    }
    let (name, exec) = (name?, exec?);
    let tokens: Vec<&str> = exec
        .split_whitespace()
        .filter(|t| !(t.len() == 2 && t.starts_with('%')))
        .collect();
    if tokens.len() != 1 {
        return None;
    }
    let prog = tokens[0].trim_matches('"').to_string();
    if prog.is_empty() {
        return None;
    }
    Some((name, prog))
}

// ---------------------------------------------------------------- launching

fn spawn_detached(mut cmd: Command) -> std::io::Result<()> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn()?;
    // Reap the child so it does not linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn launch_with(exe: &str, path: &Path) -> std::io::Result<()> {
    if path.symlink_metadata().is_err() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the file no longer exists",
        ));
    }
    #[cfg(target_os = "macos")]
    {
        if exe.ends_with(".app") {
            let mut cmd = Command::new("open");
            cmd.arg("-a").arg(exe).arg(path);
            return spawn_detached(cmd);
        }
    }
    let mut cmd = Command::new(exe);
    cmd.arg(path);
    spawn_detached(cmd)
}

pub fn default_open(path: &Path) -> std::io::Result<()> {
    open::that_detached(path)
}
