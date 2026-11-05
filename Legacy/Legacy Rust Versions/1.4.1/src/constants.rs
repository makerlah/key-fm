pub const APP_NAME: &str = "keyfm";
pub const CONFIG_FILE_NAME: &str = ".keyfm.json";
pub const OLD_PIN_FILE_NAME: &str = ".keyfm_pins.json";
pub const THIS_PC: &str = "::this-pc::";
pub const RECYCLE: &str = "::recycle-bin::";

pub const APP: &str = "keyfm";
pub const IS_WIN: bool = cfg!(windows);
pub const ALT_MASK: u32 = if cfg!(windows) { 0x20000 } else { 0x8 };
pub const VIRTUAL: &[&str] = &[THIS_PC, RECYCLE];
pub const COLS: &[&str] = &["name", "where", "size", "modified"];
pub const TITLES: &[(&str, &str)] = &[
    ("name", "Name"),
    ("where", "Original location"),
    ("size", "Size"),
    ("modified", "Modified"),
];
pub const PC_TITLES: &[(&str, Option<&str>)] = &[
    ("name", Some("Name")),
    ("where", None),
    ("size", Some("Free space")),
    ("modified", None),
];
pub const BIN_TITLES: &[(&str, &str)] = &[
    ("name", "Name"),
    ("where", "Original location"),
    ("size", "Size"),
    ("modified", "Date deleted"),
];
pub const QUICK_ICONS: &[(&str, &str)] = &[
    ("Home", "🏠"),
    ("Desktop", "🖥"),
    ("Downloads", "📥"),
    ("Documents", "🗂"),
    ("Pictures", "🖼"),
    ("Music", "🎵"),
    ("Videos", "🎬"),
    ("Recycle Bin", "🗑"),
];

pub fn title_for(col: &str) -> Option<&'static str> {
    TITLES.iter().find(|&&(c, _)| c == col).map(|&(_, t)| t)
}

pub fn pc_title_for(col: &str) -> Option<&'static str> {
    PC_TITLES.iter().find(|&&(c, _)| c == col).and_then(|&(_, t)| t)
}

pub fn bin_title_for(col: &str) -> Option<&'static str> {
    BIN_TITLES.iter().find(|&&(c, _)| c == col).map(|&(_, t)| t)
}

pub fn quick_icon(label: &str) -> Option<&'static str> {
    QUICK_ICONS.iter().find(|&&(l, _)| l == label).map(|&(_, i)| i)
}
