use std::path::{Path, PathBuf};

// ---- Windows FFI (SHFileOperationW / SHEmptyRecycleBinW) ----
#[cfg(windows)]
#[cfg_attr(target_pointer_width = "32", repr(C, packed(1)))]
#[cfg_attr(not(target_pointer_width = "32"), repr(C))]
struct SHFILEOPSTRUCTW {
    hwnd: *mut core::ffi::c_void,
    w_func: u32,
    p_from: *const u16,
    p_to: *const u16,
    f_flags: u16,
    f_any_operations_aborted: i32,
    h_name_mappings: *mut core::ffi::c_void,
    lpsz_progress_title: *const u16,
}

#[cfg(windows)]
#[link(name = "shell32")]
extern "system" {
    fn SHFileOperationW(op: *mut SHFILEOPSTRUCTW) -> i32;
    fn SHEmptyRecycleBinW(hwnd: *mut core::ffi::c_void, root: *const u16, flags: u32) -> i32;
}

/// Strict $I parser. Returns (version, original size, deleted-at unix time, original full path).
/// Returns None for truncated or malformed data instead of guessing.
pub fn parse_i_file_full(data: &[u8]) -> Option<(i64, u64, Option<f64>, String)> {
    if data.len() < 24 {
        return None;
    }
    let version = i64::from_le_bytes(data[0..8].try_into().ok()?);
    let size = i64::from_le_bytes(data[8..16].try_into().ok()?);
    let filetime = u64::from_le_bytes(data[16..24].try_into().ok()?);

    let raw: &[u8] = match version {
        1 => {
            // Version 1 is fixed-size: 24-byte header + 520-byte (260 wchar) path.
            if data.len() < 24 + 520 {
                return None;
            }
            &data[24..24 + 520]
        }
        2 => {
            if data.len() < 28 {
                return None;
            }
            let n = u32::from_le_bytes(data[24..28].try_into().ok()?) as usize;
            if n == 0 || n > 32_768 {
                return None;
            }
            let end = 28 + n * 2;
            if data.len() < end {
                return None;
            }
            &data[28..end]
        }
        _ => return None,
    };

    let utf16_units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();

    let decoded: String = char::decode_utf16(utf16_units.into_iter())
        .filter_map(|r| r.ok())
        .collect();

    let name = match decoded.find('\0') {
        Some(idx) => decoded[..idx].to_string(),
        None => decoded,
    };

    let ts = if filetime != 0 {
        Some((filetime as f64) / 10_000_000.0 - 11644473600.0)
    } else {
        None
    };

    Some((version, size.max(0) as u64, ts, name))
}

/// Backwards-compatible wrapper around [`parse_i_file_full`].
pub fn parse_i_file(data: &[u8]) -> Option<(i64, Option<f64>, String)> {
    parse_i_file_full(data).map(|(version, _size, ts, name)| (version, ts, name))
}

/// One item currently in the recycle bin.
pub struct TrashEntry {
    /// File name only.
    pub name: String,
    /// Original full path.
    pub original: String,
    /// Where the data actually lives now (the `$R...` file/folder).
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    /// Unix time of deletion.
    pub deleted: Option<f64>,
}

/// Lists the Windows recycle bin by reading `$Recycle.Bin\<sid>\$I*` on every drive.
/// Folders we are not allowed to read (other users' bins) are skipped.
#[cfg(windows)]
pub fn list_recycle_bin() -> Vec<TrashEntry> {
    let mut out = Vec::new();
    for (_label, mount) in crate::drives::scan_drives() {
        let bin_root = mount.join("$Recycle.Bin");
        let Ok(sids) = std::fs::read_dir(&bin_root) else { continue };
        for sid in sids.flatten() {
            let sid_path = sid.path();
            if !sid_path.is_dir() {
                continue;
            }
            let Ok(files) = std::fs::read_dir(&sid_path) else { continue };
            for f in files.flatten() {
                let fname = f.file_name().to_string_lossy().to_string();
                if !fname.starts_with("$I") {
                    continue;
                }
                let Ok(bytes) = std::fs::read(f.path()) else { continue };
                let Some((_version, size, ts, original)) = parse_i_file_full(&bytes) else { continue };
                let r_path = sid_path.join(format!("$R{}", &fname[2..]));
                let Ok(meta) = std::fs::symlink_metadata(&r_path) else { continue };
                let name = Path::new(&original)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| original.clone());
                out.push(TrashEntry {
                    name,
                    original,
                    path: r_path,
                    is_dir: meta.is_dir(),
                    size,
                    deleted: ts,
                });
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub fn list_recycle_bin() -> Vec<TrashEntry> {
    Vec::new()
}

// ---------------------------------------------------------------------------
// Recycle bin / trash support
// ---------------------------------------------------------------------------

/// Move a file or directory to the OS recycle bin (Windows) / trash
/// (macOS/Linux).
pub fn move_to_trash(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        win_move_to_recycle_bin(path)
    }
    #[cfg(target_os = "macos")]
    {
        mac_move_to_trash(path)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux_move_to_trash(path)
    }
}

/// Empty the OS recycle bin. On non-Windows this removes the contents of
/// the local trash directories.
pub fn win_empty_recycle_bin() -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND
        let flags: u32 = 0x1 | 0x2 | 0x4;
        let rc = unsafe { SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), flags) };
        // This returns an HRESULT, not a GetLastError value. S_OK / S_FALSE mean
        // success; E_UNEXPECTED (0x8000FFFF) is what Windows reports when the
        // bin is already empty, which is not a failure for the user.
        if rc == 0 || rc == 1 || (rc as u32) == 0x8000_FFFF {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("Empty Recycle Bin failed (HRESULT 0x{:X})", rc as u32)))
        }
    }
    #[cfg(not(windows))]
    {
        empty_local_trash()
    }
}

// ---- Windows ----
#[cfg(windows)]
fn win_move_to_recycle_bin(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    from.push(0);
    from.push(0);

    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        w_func: 3, // FO_DELETE
        p_from: from.as_ptr(),
        p_to: std::ptr::null(),
        // FOF_ALLOWUNDO | FOF_SILENT | FOF_NOERRORUI | FOF_WANT_NUKE_WARNING
        // (FOF_NOCONFIRMATION is deliberately NOT set, otherwise items that
        // cannot be recycled would be deleted permanently without a prompt.)
        f_flags: 0x0040 | 0x0004 | 0x0400 | 0x4000,
        f_any_operations_aborted: 0,
        h_name_mappings: std::ptr::null_mut(),
        lpsz_progress_title: std::ptr::null(),
    };

    let rc = unsafe { SHFileOperationW(&mut op) };
    // SHFileOperationW returns its own error codes (not GetLastError values).
    let aborted = op.f_any_operations_aborted;
    if rc == 0 && aborted == 0 {
        Ok(())
    } else if aborted != 0 {
        Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "Cancelled"))
    } else {
        Err(std::io::Error::other(format!("Shell delete failed (code 0x{:X})", rc)))
    }
}

// ---- macOS ----
#[cfg(target_os = "macos")]
fn mac_move_to_trash(path: &Path) -> std::io::Result<()> {
    let home = home_dir()?;
    let trash = home.join(".Trash");
    std::fs::create_dir_all(&trash)?;
    unique_move(path, &trash)
}

// ---- Linux / other unix (freedesktop.org trash spec) ----
#[cfg(all(unix, not(target_os = "macos")))]
fn linux_move_to_trash(path: &Path) -> std::io::Result<()> {
    use std::io::Write;

    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().unwrap_or_default().join(".local/share"));
    let trash = data_home.join("Trash");
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;

    let base_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name"))?;

    // Canonicalize the PARENT and re-append the name, so a symlink is recorded
    // by its own location instead of the location of its target.
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let original = parent.join(&base_name).to_string_lossy().to_string();

    // Claim a unique name by atomically creating the .trashinfo file first.
    let mut n = 0u32;
    let (dest, info_path) = loop {
        let candidate = if n == 0 { base_name.clone() } else { format!("{}.{}", base_name, n) };
        let dest = files.join(&candidate);
        let info_path = info.join(format!("{}.trashinfo", candidate));
        if dest.symlink_metadata().is_err() {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&info_path) {
                Ok(mut f) => {
                    let body = format!(
                        "[Trash Info]\nPath={}\nDeletionDate={}\n",
                        percent_encode(&original),
                        now_local_iso8601(),
                    );
                    if let Err(e) = f.write_all(body.as_bytes()) {
                        drop(f);
                        let _ = std::fs::remove_file(&info_path);
                        return Err(e);
                    }
                    break (dest, info_path);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        n += 1;
    };

    // Move the data; never leave an orphaned .trashinfo behind on failure.
    // Across filesystems this falls back to copy + delete.
    if let Err(e) = crate::helpers::move_path(path, &dest) {
        let _ = std::fs::remove_file(&info_path);
        return Err(e);
    }
    Ok(())
}

#[cfg(not(windows))]
fn empty_local_trash() -> std::io::Result<()> {
    let mut targets: Vec<PathBuf> = Vec::new();
    if let Ok(home) = home_dir() {
        targets.push(home.join(".Trash"));
        targets.push(home.join(".local/share/Trash/files"));
        targets.push(home.join(".local/share/Trash/info"));
    }
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        let t = PathBuf::from(x).join("Trash");
        targets.push(t.join("files"));
        targets.push(t.join("info"));
    }
    for dir in targets {
        if !dir.is_dir() {
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                let _ = if p.is_dir() {
                    std::fs::remove_dir_all(&p)
                } else {
                    std::fs::remove_file(&p)
                };
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn home_dir() -> std::io::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set"))
}

/// Move `path` into `dir`, appending `.1`, `.2`, … if a name collision
/// already exists so we never overwrite another trashed item. Works across
/// filesystems (copy + delete fallback).
#[cfg(target_os = "macos")]
fn unique_move(path: &Path, dir: &Path) -> std::io::Result<()> {
    let base = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name"))?;
    let mut dest: PathBuf = dir.join(&base);
    let mut n = 1;
    while dest.symlink_metadata().is_ok() {
        dest = dir.join(format!("{}.{}", base, n));
        n += 1;
    }
    crate::helpers::move_path(path, &dest)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn percent_encode(s: &str) -> String {
    // Minimal percent-encoding for characters that would otherwise break
    // the .trashinfo format (spaces, newlines, '=', '%').
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/');
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
fn now_local_iso8601() -> String {
    // The trash spec requires local time, without a timezone suffix.
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}
