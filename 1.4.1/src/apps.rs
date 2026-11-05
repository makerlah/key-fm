use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
use winreg::enums::*;
#[cfg(windows)]
use winreg::RegKey;

static APPS_CACHE: OnceLock<Vec<(String, String)>> = OnceLock::new();

pub fn list_apps() -> Vec<(String, String)> {
    APPS_CACHE
        .get_or_init(|| {
            #[cfg(windows)]
            {
                let mut apps: Vec<(String, String)> = Vec::new();
                let roots = [
                    (HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths"),
                    (HKEY_CURRENT_USER, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths"),
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
                        let Ok(exe_path) = exe_key.get_value::<String, _>("") else {
                            continue;
                        };

                        let cleaned = exe_path.trim_matches('"').to_string();
                        if cleaned.is_empty() {
                            continue;
                        }

                        let name = exe_name.trim_end_matches(".exe").to_string();
                        apps.push((name, cleaned));
                    }
                }

                apps.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                apps.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
                apps
            }
            #[cfg(not(windows))]
            {
                Vec::new()
            }
        })
        .clone()
}

pub fn launch_with(exe: &str, path: &Path) -> std::io::Result<()> {
    let mut cmd = Command::new(exe);
    cmd.arg(path);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn default_open(path: &Path) -> std::io::Result<()> {
    open::that_detached(path)
}