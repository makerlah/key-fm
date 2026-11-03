use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use crate::constants::{CONFIG_FILE_NAME, OLD_PIN_FILE_NAME};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AssocEntry {
    pub name: String,
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default = "default_dark")]
    pub dark: bool,
    #[serde(default)]
    pub show_hidden: bool,
    #[serde(default)]
    pub pins: Vec<String>,
    #[serde(default)]
    pub unpinned: Vec<String>,
    #[serde(default)]
    pub assoc: HashMap<String, AssocEntry>,
    #[serde(default)]
    pub keys: HashMap<String, String>,
}

fn default_dark() -> bool {
    false
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dark: false,
            show_hidden: false,
            pins: Vec::new(),
            unpinned: Vec::new(),
            assoc: HashMap::new(),
            keys: HashMap::new(),
        }
    }
}

pub fn config_path() -> Option<PathBuf> {
    if let Some(proj_dirs) = ProjectDirs::from("", "", "keyfm") {
        let dir = proj_dirs.config_dir();
        let _ = fs::create_dir_all(dir);
        // Python app uses ~/.keyfm.json directly via home_dir
    }
    if let Some(ud) = directories::UserDirs::new() {
        return Some(ud.home_dir().join(CONFIG_FILE_NAME));
    }
    None
}

pub fn old_pins_path() -> Option<PathBuf> {
    directories::UserDirs::new().map(|ud| ud.home_dir().join(OLD_PIN_FILE_NAME))
}

pub fn load_config() -> Config {
    let mut cfg = Config::default();
    
    // Migrate old pins file if present (F-25)
    if let Some(old_path) = old_pins_path() {
        if old_path.exists() {
            if let Ok(data) = fs::read_to_string(&old_path) {
                if let Ok(old_pins) = serde_json::from_str::<Vec<String>>(&data) {
                    cfg.pins.extend(old_pins);
                }
            }
            let _ = fs::remove_file(old_path);
        }
    }

    if let Some(path) = config_path() {
        if path.exists() {
            if let Ok(bytes) = fs::read(&path) {
                // Handle UTF-8 BOM if present (F-24)
                let content = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
                    String::from_utf8_lossy(&bytes[3:]).to_string()
                } else {
                    String::from_utf8_lossy(&bytes).to_string()
                };

                if let Ok(parsed) = serde_json::from_str::<Config>(&content) {
                    cfg = parsed;
                }
            }
        }
    }

    // Validate assoc entries (F-8)
    cfg.assoc.retain(|_, entry| {
        !entry.name.trim().is_empty() && !entry.argv.is_empty() && entry.argv.iter().all(|s| !s.is_empty())
    });

    cfg
}

pub fn save_config(cfg: &Config) {
    if let Some(path) = config_path() {
        if let Ok(json) = serde_json::to_string_pretty(cfg) {
            let temp_path = path.with_extension("tmp");
            if fs::write(&temp_path, json).is_ok() {
                let _ = fs::rename(&temp_path, &path);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
                }
            }
        }
    }
}
