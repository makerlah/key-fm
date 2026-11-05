use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct ActionDef {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub default_keys: &'static [&'static str],
    pub scope: &'static str,
}

pub const ACTION_DEFS: &[ActionDef] = &[
    ActionDef { id: "up", label: "Move up", group: "Navigation", default_keys: &["Up", "k"], scope: "list" },
    ActionDef { id: "down", label: "Move down", group: "Navigation", default_keys: &["Down", "j"], scope: "list" },
    ActionDef { id: "page_up", label: "Page up", group: "Navigation", default_keys: &["Prior", "Ctrl+u"], scope: "list" },
    ActionDef { id: "page_down", label: "Page down", group: "Navigation", default_keys: &["Next", "Ctrl+d"], scope: "list" },
    ActionDef { id: "home", label: "First item", group: "Navigation", default_keys: &["Home", "g g"], scope: "list" },
    ActionDef { id: "end", label: "Last item", group: "Navigation", default_keys: &["End", "G"], scope: "list" },
    ActionDef { id: "open", label: "Open item / enter directory", group: "Navigation", default_keys: &["Return", "Right", "l"], scope: "list" },
    ActionDef { id: "back", label: "Go back", group: "Navigation", default_keys: &["BackSpace", "Left", "h"], scope: "list" },
    ActionDef { id: "parent", label: "Go to parent directory", group: "Navigation", default_keys: &["Shift+BackSpace"], scope: "list" },
    ActionDef { id: "root", label: "Go to root / This PC", group: "Navigation", default_keys: &["Ctrl+Shift+r"], scope: "global" },
    ActionDef { id: "recycle", label: "Go to Recycle Bin", group: "Navigation", default_keys: &["Ctrl+Shift+b"], scope: "global" },
    ActionDef { id: "cd", label: "Change directory (prompt)", group: "Navigation", default_keys: &["c"], scope: "list" },
    ActionDef { id: "search", label: "Search files", group: "Navigation", default_keys: &["/"], scope: "list" },
    ActionDef { id: "quick_jump", label: "Quick jump (pins/drives)", group: "Navigation", default_keys: &["q"], scope: "list" },

    ActionDef { id: "select", label: "Toggle selection", group: "Selection", default_keys: &["space"], scope: "list" },
    ActionDef { id: "select_all", label: "Select all", group: "Selection", default_keys: &["Ctrl+a"], scope: "list" },
    ActionDef { id: "invert_selection", label: "Invert selection", group: "Selection", default_keys: &["Ctrl+i"], scope: "list" },
    ActionDef { id: "clear_selection", label: "Clear selection", group: "Selection", default_keys: &["Escape"], scope: "list" },
    ActionDef { id: "visual_line", label: "Visual line mode", group: "Selection", default_keys: &["V"], scope: "list" },

    ActionDef { id: "copy", label: "Copy paths to clipboard", group: "File Ops", default_keys: &["y"], scope: "list" },
    ActionDef { id: "cut", label: "Cut paths to clipboard", group: "File Ops", default_keys: &["x"], scope: "list" },
    ActionDef { id: "paste", label: "Paste from clipboard", group: "File Ops", default_keys: &["p"], scope: "list" },
    ActionDef { id: "delete", label: "Delete (to trash/recycle)", group: "File Ops", default_keys: &["Delete", "d d"], scope: "list" },
    ActionDef { id: "permanent_delete", label: "Delete permanently", group: "File Ops", default_keys: &["Shift+Delete"], scope: "list" },
    ActionDef { id: "rename", label: "Rename item", group: "File Ops", default_keys: &["r"], scope: "list" },
    ActionDef { id: "new_file", label: "New file", group: "File Ops", default_keys: &["n"], scope: "list" },
    ActionDef { id: "new_dir", label: "New directory", group: "File Ops", default_keys: &["Shift+n"], scope: "list" },
    ActionDef { id: "properties", label: "File properties", group: "File Ops", default_keys: &["i"], scope: "list" },

    ActionDef { id: "open_default", label: "Open with default application", group: "Execute", default_keys: &["o"], scope: "list" },
    ActionDef { id: "open_with", label: "Open with... (chooser)", group: "Execute", default_keys: &["O"], scope: "list" },
    ActionDef { id: "terminal", label: "Open terminal here", group: "Execute", default_keys: &["t"], scope: "list" },

    ActionDef { id: "toggle_hidden", label: "Toggle hidden files", group: "View", default_keys: &["."], scope: "global" },
    ActionDef { id: "toggle_dark", label: "Toggle dark mode", group: "View", default_keys: &["F2"], scope: "global" },
    ActionDef { id: "sort_name", label: "Sort by name", group: "View", default_keys: &["s n"], scope: "global" },
    ActionDef { id: "sort_size", label: "Sort by size", group: "View", default_keys: &["s s"], scope: "global" },
    ActionDef { id: "sort_mtime", label: "Sort by modified time", group: "View", default_keys: &["s m"], scope: "global" },
    ActionDef { id: "sort_reverse", label: "Toggle sort direction", group: "View", default_keys: &["s r"], scope: "global" },

    ActionDef { id: "pin", label: "Pin / unpin current folder", group: "Bookmarks", default_keys: &["P"], scope: "list" },
    ActionDef { id: "sidebar_focus", label: "Focus sidebar / file list", group: "View", default_keys: &["Tab"], scope: "global" },

    ActionDef { id: "help", label: "Show keyboard help", group: "Help", default_keys: &["F1", "?"], scope: "global" },
    ActionDef { id: "refresh", label: "Refresh view", group: "Navigation", default_keys: &["F5", "Ctrl+r"], scope: "global" },
    ActionDef { id: "quit", label: "Quit application", group: "System", default_keys: &["Ctrl+q", "Ctrl+w"], scope: "global" },
    ActionDef { id: "command_palette", label: "Command palette", group: "System", default_keys: &["Ctrl+Shift+p", ":"], scope: "global" },
];

pub fn action_info(id: &str) -> Option<&'static ActionDef> {
    ACTION_DEFS.iter().find(|a| a.id == id)
}

pub const ENTRY_PASS: &[&str] = &["hidden"];

pub const FIXED_KEYS: &[(&str, &str)] = &[
    ("F1", "help"),
    ("F2", "toggle_dark"),
    ("F5", "refresh"),
    ("Ctrl+q", "quit"),
    ("Ctrl+w", "quit"),
    ("Escape", "clear_selection"),
    ("Tab", "sidebar_focus"),
];

pub const MOD_ORDER: &[&str] = &["Ctrl", "Alt", "Shift"];

pub fn get_key_aliases() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("Return", "Enter");
    m.insert("Prior", "PgUp");
    m.insert("Next", "PgDown");
    m.insert("Escape", "Esc");
    m.insert("BackSpace", "Backspace");
    m
}

pub fn get_key_display() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("enter", "Enter");
    m.insert("return", "Enter");
    m.insert("space", "Space");
    m.insert("prior", "PgUp");
    m.insert("next", "PgDown");
    m.insert("escape", "Esc");
    m.insert("backspace", "Backspace");
    m
}

pub fn get_mod_syms() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("Ctrl", "Control");
    m.insert("Alt", "Alt");
    m.insert("Shift", "Shift");
    m
}

pub fn split_key(spec: &str) -> (Vec<&'static str>, String) {
    let parts: Vec<&str> = spec.split('+').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return (Vec::new(), String::new());
    }
    let mut mods = Vec::new();
    let last = parts.last().unwrap_or(&"");
    let mod_candidates = &parts[..parts.len() - 1];
    for m in mod_candidates {
        let mut cap = m.to_lowercase();
        if let Some(r) = cap.get_mut(0..1) {
            r.make_ascii_uppercase();
        }
        for &mo in MOD_ORDER {
            if mo.eq_ignore_ascii_case(&cap) || mo == *m {
                if !mods.contains(&mo) {
                    mods.push(mo);
                }
                break;
            }
        }
    }
    let mut ordered_mods = Vec::new();
    for &mo in MOD_ORDER {
        if mods.contains(&mo) {
            ordered_mods.push(mo);
        }
    }
    let key = if parts.len() == 1 && MOD_ORDER.contains(&parts[0]) {
        String::new()
    } else if parts.len() == 1 {
        parts[0].to_string()
    } else {
        last.to_string()
    };
    (ordered_mods, key)
}

pub fn norm_spec(spec: &str) -> String {
    let spec = spec.trim();
    let (mods, key) = split_key(spec);
    if key.is_empty() {
        return String::new();
    }
    let aliases = get_key_aliases();
    let display = get_key_display();
    let key_str = key.as_str();
    let lookup_key = aliases.get(key_str).unwrap_or(&key_str);
    let final_key = display.get(*lookup_key).unwrap_or(lookup_key);    
    let key_str = if final_key.len() == 1 && final_key.chars().all(char::is_alphabetic) {
        final_key.to_uppercase()
    } else {
        final_key.to_string()
    };

    let mut result_parts = Vec::new();
    for m in mods {
        result_parts.push(m.to_string());
    }
    result_parts.push(key_str);
    result_parts.join("+")
}

pub fn is_safe_key(spec: &str) -> bool {
    let (mods, key) = split_key(spec);
    if mods.contains(&"Ctrl") || mods.contains(&"Alt") {
        return true;
    }
    if key.starts_with('F') && key.len() >= 2 && key.len() <= 3 {
        let num_part = &key[1..];
        if num_part.chars().all(char::is_numeric) {
            if let Ok(n) = num_part.parse::<u32>() {
                if n >= 1 && n <= 99 {
                    return true;
                }
            }
        }
    }
    false
}

pub fn key_sequences(spec: &str) -> Vec<String> {
    let (mods, key) = split_key(spec);
    let mut prefix = String::new();
    for m in MOD_ORDER {
        if mods.contains(m) {
            if *m == "Ctrl" {
                prefix.push_str("Control-");
            } else if *m == "Alt" {
                prefix.push_str("Alt-");
            } else if *m == "Shift" {
                prefix.push_str("Shift-");
            }
        }
    }
    let aliases = get_key_aliases();
    let key_str = key.as_str();
    let keysym = aliases.get(key_str).unwrap_or(&key_str);    let mut syms = Vec::new();
    if keysym.len() == 1 && keysym.chars().all(char::is_alphabetic) {
        syms.push(keysym.to_lowercase());
        syms.push(keysym.to_uppercase());
    } else {
        syms.push(keysym.to_string());
    }
    if *keysym == "Return" {
        syms.push("KP_Enter".to_string());
    }
    if *keysym == "Tab" && mods.contains(&"Shift") {
        syms.push("ISO_Left_Tab".to_string());
    }
    if *keysym == "Menu" {
        syms.push("App".to_string());
    }

    let mut res = Vec::new();
    for sym in syms {
        res.push(format!("<{}Key-{}>", prefix, sym));
    }
    res
}

pub fn get_reserved() -> Vec<String> {
    let mut res = Vec::new();
    for &(k, _) in FIXED_KEYS {
        res.push(norm_spec(k));
    }
    for i in 1..=9 {
        res.push(format!("Alt+{}", i));
    }
    for &ch in &['F', 'E', 'V', 'G', 'S'] {
        res.push(format!("Alt+{}", ch));
    }
    res
}
