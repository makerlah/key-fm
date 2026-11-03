use std::path::PathBuf;
use sysinfo::Disks;

pub fn scan_drives() -> Vec<(String, PathBuf)> {
    let mut drives = Vec::new();
    let disks = Disks::new_with_refreshed_list();
    for disk in &disks {
        let mount = disk.mount_point();
        let name = disk.name().to_string_lossy();
        let mount_str = mount.to_string_lossy();
        let label = if name.is_empty() {
            format!("Local Disk ({})", mount_str.trim_end_matches(['/', '\\']))
        } else {
            format!("{} ({})", name, mount_str.trim_end_matches(['/', '\\']))
        };
        drives.push((label, mount.to_path_buf()));
    }
    if drives.is_empty() {
        drives.push(("Root".to_string(), PathBuf::from("/")));
    }
    drives
}

pub fn known_folders() -> Vec<(&'static str, PathBuf)> {
    let mut folders = Vec::new();
    if let Some(ud) = directories::UserDirs::new() {
        folders.push(("Home", ud.home_dir().to_path_buf()));
        if let Some(p) = ud.desktop_dir() { folders.push(("Desktop", p.to_path_buf())); }
        if let Some(p) = ud.document_dir() { folders.push(("Documents", p.to_path_buf())); }
        if let Some(p) = ud.download_dir() { folders.push(("Downloads", p.to_path_buf())); }
        if let Some(p) = ud.picture_dir() { folders.push(("Pictures", p.to_path_buf())); }
        if let Some(p) = ud.audio_dir() { folders.push(("Music", p.to_path_buf())); }
        if let Some(p) = ud.video_dir() { folders.push(("Videos", p.to_path_buf())); }
    }
    #[cfg(windows)]
    {
        folders.push(("Recycle Bin", PathBuf::from("::recycle-bin::")));
    }
    folders
}
