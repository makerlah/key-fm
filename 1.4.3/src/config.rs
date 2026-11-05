use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use crate::constants::{CONFIG_FILE_NAME, OLD_PIN_FILE_NAME};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AssocEntry {
    pub name: String,
    pub argv: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Config {
    pub dark: bool,
    pub show_hidden: bool,
    pub pins: Vec<String>,
    pub unpinned: Vec<String>,
    pub assoc: HashMap<String, AssocEntry>,
    pub keys: HashMap<String, Vec<String>>,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
    #[serde(default = "default_size_col")]
    pub size_col_width: f32,
    #[serde(default = "default_mod_col")]
    pub mod_col_width: f32,
}

pub fn default_sidebar_width() -> f32 {
    220.0
}

pub fn default_size_col() -> f32 {
    100.0
}

pub fn default_mod_col() -> f32 {
    150.0
}

pub fn config_path() -> Option<PathBuf> {
    if let Some(ud) = directories::UserDirs::new() {
        return Some(ud.home_dir().join(CONFIG_FILE_NAME));
    }
    None
}

pub fn old_pins_path() -> Option<PathBuf> {
    directories::UserDirs::new().map(|ud| ud.home_dir().join(OLD_PIN_FILE_NAME))
}

/// Loads the config. The returned flag is true when the pins were migrated
/// from the legacy pin file; the legacy file is NOT deleted here, so nothing
/// is lost unless the caller has first saved the new config successfully.
pub fn load_config_with_migration(path: &Path, old_pins: &Path) -> (Config, bool) {
    let (cfg, migrated, _corrupt) = load_config_full(path, old_pins);
    (cfg, migrated)
}

/// Like [`load_config_with_migration`], plus a flag that is true when the main
/// config file exists but could not be parsed (defaults were used instead).
pub fn load_config_full(path: &Path, old_pins: &Path) -> (Config, bool, bool) {
    let mut cfg = Config::default();
    let mut migrated = false;

    let mut loaded_main = false;
    if path.exists() {
        if let Ok(bytes) = fs::read(path) {
            let content = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
                String::from_utf8_lossy(&bytes[3..]).to_string()
            } else {
                String::from_utf8_lossy(&bytes).to_string()
            };

            if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(&content) {
                loaded_main = true;
                if let Some(v) = map.get("dark") {
                    if let Some(b) = v.as_bool() {
                        cfg.dark = b;
                    }
                }
                if let Some(v) = map.get("show_hidden") {
                    if let Some(b) = v.as_bool() {
                        cfg.show_hidden = b;
                    }
                }
                if let Some(v) = map.get("pins") {
                    if let Some(arr) = v.as_array() {
                        let mut pins = Vec::new();
                        for item in arr {
                            if let Some(s) = item.as_str() {
                                pins.push(s.to_string());
                            }
                        }
                        cfg.pins = pins;
                    }
                }
                if let Some(v) = map.get("unpinned") {
                    if let Some(arr) = v.as_array() {
                        let mut unpinned = Vec::new();
                        for item in arr {
                            if let Some(s) = item.as_str() {
                                unpinned.push(s.to_string());
                            }
                        }
                        cfg.unpinned = unpinned;
                    }
                }
                if let Some(v) = map.get("assoc") {
                    if let Some(obj) = v.as_object() {
                        let mut assoc = HashMap::new();
                        for (k, val) in obj {
                            if let Some(sub_obj) = val.as_object() {
                                if let Some(name_v) = sub_obj.get("name") {
                                    if let Some(name) = name_v.as_str() {
                                        if !name.trim().is_empty() {
                                            if let Some(argv_v) = sub_obj.get("argv") {
                                                if let Some(arr) = argv_v.as_array() {
                                                    let mut argv = Vec::new();
                                                    let mut valid = true;
                                                    for arg_item in arr {
                                                        if let Some(s) = arg_item.as_str() {
                                                            if s.is_empty() {
                                                                valid = false;
                                                                break;
                                                            }
                                                            argv.push(s.to_string());
                                                        } else {
                                                            valid = false;
                                                            break;
                                                        }
                                                    }
                                                    if valid && !argv.is_empty() {
                                                        let cwd = sub_obj.get("cwd").and_then(|c| c.as_str()).map(|s| s.to_string());
                                                        assoc.insert(k.clone(), AssocEntry { name: name.to_string(), argv, cwd });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        cfg.assoc = assoc;
                    }
                }
                if let Some(v) = map.get("keys") {
                    if let Some(obj) = v.as_object() {
                        let mut keys = HashMap::new();
                        for (action_id, val) in obj {
                            if crate::keys::action_info(action_id).is_some() {
                                if let Some(arr) = val.as_array() {
                                    let mut specs = Vec::new();
                                    for spec_v in arr {
                                        if let Some(spec_str) = spec_v.as_str() {
                                            let norm = crate::keys::norm_spec(spec_str);
                                            if !norm.is_empty() && !specs.contains(&norm) {
                                                specs.push(norm);
                                            }
                                        }
                                    }
                                    if !specs.is_empty() {
                                        keys.insert(action_id.clone(), specs);
                                    }
                                }
                            }
                        }
                        cfg.keys = keys;
                    }
                }
            }
        }
    }

    if !loaded_main {
        if old_pins.exists() {
            if let Ok(data) = fs::read_to_string(old_pins) {
                if let Ok(old_pins_list) = serde_json::from_str::<Vec<String>>(&data) {
                    cfg.pins = old_pins_list;
                    migrated = true;
                }
            }
        }
    }

    let corrupt = !loaded_main && path.exists();
    (cfg, migrated, corrupt)
}

pub fn load_config_from(path: &Path, old_pins: &Path) -> Config {
    load_config_with_migration(path, old_pins).0
}

pub fn save_config_to(cfg: &Config, path: &Path) -> bool {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let temp_path = path.with_extension("tmp");
        if fs::write(&temp_path, json).is_ok() {
            // rename can fail across filesystems (NFS, symlinked $HOME): fall back to copy.
            let placed = fs::rename(&temp_path, path).is_ok()
                || (fs::copy(&temp_path, path).is_ok() && {
                    let _ = fs::remove_file(&temp_path);
                    true
                });
            if placed {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
                }
                return true;
            }
            let _ = fs::remove_file(&temp_path);
        }
    }
    false
}

pub fn load_config() -> Config {
    load_config_status().0
}

/// Loads the config; the second value is a message for the user when something
/// needed attention (an unreadable config was backed up and defaults are in use).
pub fn load_config_status() -> (Config, Option<String>) {
    let p = config_path().unwrap_or_else(|| PathBuf::from(CONFIG_FILE_NAME));
    let op = old_pins_path().unwrap_or_else(|| PathBuf::from(OLD_PIN_FILE_NAME));
    let (cfg, migrated, corrupt) = load_config_full(&p, &op);
    let mut notice = None;
    if (migrated || corrupt) && p.exists() {
        // Keep a copy of an unreadable main config rather than silently overwriting it.
        let bak = p.with_extension("bak");
        if fs::copy(&p, &bak).is_ok() && corrupt {
            notice = Some(format!(
                "Config could not be read; using defaults. A copy was saved to {}",
                bak.display()
            ));
        }
    }
    if migrated {
        // Only remove the legacy pin file once the new config is safely on disk.
        if save_config_to(&cfg, &p) {
            let _ = fs::remove_file(&op);
        }
    }
    (cfg, notice)
}

pub fn save_config(cfg: &Config) -> bool {
    if let Some(p) = config_path() {
        return save_config_to(cfg, &p);
    }
    false
}
