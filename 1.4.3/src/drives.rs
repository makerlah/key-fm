use std::path::PathBuf;
use sysinfo::Disks;

/// On Windows, only accept real drive letters like "C:\".
/// Folder mounts (like that PortableBaseLayer thing) are skipped.
fn is_listable_mount(mount: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        let s = mount.to_string_lossy();
        let b = s.as_bytes();
        b.len() >= 2
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && s[2..].trim_matches(['\\', '/']).is_empty()
    }
    #[cfg(not(windows))]
    {
        let _ = mount;
        true
    }
}

/// (label, mount point, free bytes, total bytes)
pub fn scan_drives_info() -> Vec<(String, PathBuf, u64, u64)> {
    let mut drives = Vec::new();
    let disks = Disks::new_with_refreshed_list();
    for disk in &disks {
        let mount = disk.mount_point();
        if !is_listable_mount(mount) {
            continue;
        }
        let name = disk.name().to_string_lossy();
        let mount_str = mount.to_string_lossy();
        let label = if name.is_empty() {
            format!("Local Disk ({})", mount_str.trim_end_matches(['/', '\\']))
        } else {
            format!("{} ({})", name, mount_str.trim_end_matches(['/', '\\']))
        };
        drives.push((label, mount.to_path_buf(), disk.available_space(), disk.total_space()));
    }
    if drives.is_empty() {
        drives.push(("Root".to_string(), PathBuf::from("/"), 0, 0));
    }
    drives
}

pub fn scan_drives() -> Vec<(String, PathBuf)> {
    scan_drives_info()
        .into_iter()
        .map(|(label, path, _, _)| (label, path))
        .collect()
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