use std::cmp::min;
use std::path::{Path, PathBuf};

// ---- Windows FFI (SHFileOperationW / SHEmptyRecycleBinW) ----
#[cfg(windows)]
#[repr(C)]
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

pub fn parse_i_file(data: &[u8]) -> Option<(i64, Option<f64>, String)> {
    if data.len() < 24 {
        return None;
    }
    let version = i64::from_le_bytes(data[0..8].try_into().ok()?);
    let _size = i64::from_le_bytes(data[8..16].try_into().ok()?);
    let filetime = u64::from_le_bytes(data[16..24].try_into().ok()?);

    let raw = match version {
        1 => {
            if data.len() < 24 {
                return None;
            }
            let end = min(24 + 520, data.len());
            &data[24..end]
        }
        2 => {
            if data.len() < 28 {
                return None;
            }
            let n = u32::from_le_bytes(data[24..28].try_into().ok()?) as usize;
            let start = 28;
            let end = min(start + n * 2, data.len());
            &data[start..end]
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

    Some((version, ts, name))
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
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
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
        // FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI
        f_flags: 0x0040 | 0x0010 | 0x0004 | 0x0400,
        f_any_operations_aborted: 0,
        h_name_mappings: std::ptr::null_mut(),
        lpsz_progress_title: std::ptr::null(),
    };

    let rc = unsafe { SHFileOperationW(&mut op) };
    if rc == 0 && op.f_any_operations_aborted == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
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
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().unwrap_or_default().join(".local/share"));
    let trash = data_home.join("Trash");
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;

    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let original = abs.to_string_lossy().to_string();

    // Compute a unique destination name inside files/.
    let base_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".to_string());
    let mut dest = files.join(&base_name);
    let mut n = 1;
    while dest.exists() {
        dest = files.join(format!("{}.{}", base_name, n));
        n += 1;
    }

    // Write the .trashinfo file describing the original location.
    let dest_name = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let info_path = info.join(format!("{}.trashinfo", dest_name));
    let info_body = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent_encode(&original),
        now_iso8601(),
    );
    std::fs::write(&info_path, info_body)?;

    std::fs::rename(path, &dest)
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

fn home_dir() -> std::io::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set"))
}

/// Move `path` into `dir`, appending `.1`, `.2`, … if a name collision
/// already exists so we never overwrite another trashed item.
#[cfg(unix)]
fn unique_move(path: &Path, dir: &Path) -> std::io::Result<()> {
    use std::path::PathBuf;
    let base = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name"))?;
    let mut dest: PathBuf = dir.join(&base);
    let mut n = 1;
    while dest.exists() {
        dest = dir.join(format!("{}.{}", base, n));
        n += 1;
    }
    std::fs::rename(path, &dest)
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
fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // days since epoch
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", y, mo, d, h, m, s)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
