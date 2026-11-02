"""Drive discovery and known-folder resolution."""
import os
import shutil
import sys

from .constants import IS_WIN, RECYCLE
from .helpers import human_size
from .models import Item


def win_drive_roots(kinds=(2, 3)):
    """Enumerate drive roots of the given drive-type kinds (used by trash.py)."""
    import ctypes
    k32 = ctypes.windll.kernel32
    mask = k32.GetLogicalDrives()
    out = []
    for i in range(26):
        if mask & (1 << i):
            root = f"{chr(65 + i)}:\\"
            if k32.GetDriveTypeW(root) in kinds:
                out.append(root)
    return out


def _xdg_user_dirs():
    """Parse ~/.config/user-dirs.dirs (or $XDG_CONFIG_HOME) on POSIX (#11)."""
    home = os.path.expanduser("~")
    cfg_home = os.environ.get("XDG_CONFIG_HOME") or os.path.join(home, ".config")
    path = os.path.join(cfg_home, "user-dirs.dirs")
    out = {}
    if not os.path.isfile(path):
        return out
    mapping = {
        "XDG_DESKTOP_DIR": "Desktop",
        "XDG_DOWNLOAD_DIR": "Downloads",
        "XDG_DOCUMENTS_DIR": "Documents",
        "XDG_PICTURES_DIR": "Pictures",
        "XDG_MUSIC_DIR": "Music",
        "XDG_VIDEOS_DIR": "Videos",
    }
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#") or "=" not in line:
                    continue
                k, v = line.split("=", 1)
                k = k.strip()
                if k not in mapping:
                    continue
                v = v.strip()
                if len(v) >= 2 and v[0] == '"' and v[-1] == '"':
                    v = v[1:-1]
                v = v.replace("${HOME}", home).replace("$HOME", home)
                if v:
                    out[mapping[k]] = v
    except OSError:
        pass
    return out


def scan_drives(quick=False):
    items = []
    if IS_WIN:
        import ctypes
        k32 = ctypes.windll.kernel32
        mask = k32.GetLogicalDrives()
        # #31: RAM disk
        defaults = {2: "Removable Disk", 3: "Local Disk", 4: "Network Drive",
                    5: "CD Drive", 6: "RAM Disk"}
        for i in range(26):
            if not mask & (1 << i):
                continue
            letter = chr(65 + i)
            root = f"{letter}:\\"
            dtype = k32.GetDriveTypeW(root)
            label, extra = "", ""
            if not (quick and dtype == 4):
                try:
                    buf = ctypes.create_unicode_buffer(261)
                    if k32.GetVolumeInformationW(root, buf, 261, None, None,
                                                 None, None, 0):
                        label = buf.value
                except Exception:
                    pass
            if not quick:
                try:
                    total, _used, free = shutil.disk_usage(root)
                    used = total - free
                    percent = (used / total * 100) if total else 0
                    extra = (f"{human_size(used)} used of {human_size(total)} "
                             f"({percent:.1f}%)")
                except OSError:
                    pass
            name = f"{label or defaults.get(dtype, 'Disk')} ({letter}:)"
            items.append(Item(name, root, True, 0, None, False, extra))
    else:
        cands = [("File system (/)", "/")]
        # #32: dedupe /Volumes/* entries that are the same fs as /
        try:
            root_stat = os.stat("/")
            root_dev, root_ino = root_stat.st_dev, root_stat.st_ino
        except OSError:
            root_dev = root_ino = None
        # #12: empty $HOME basename (root user) must not add /media/ and /run/media/
        user = os.path.basename(os.path.expanduser("~").rstrip("/"))
        if sys.platform == "darwin":
            bases = ["/Volumes"]
        else:
            bases = []
            if user:
                bases += [f"/media/{user}", f"/run/media/{user}"]
            bases.append("/mnt")
        for b in bases:
            try:
                for e in sorted(os.listdir(b)):
                    full = os.path.join(b, e)
                    if not os.path.isdir(full):
                        continue
                    if root_dev is not None:
                        try:
                            st = os.stat(full)
                            if st.st_dev == root_dev and st.st_ino == root_ino:
                                continue
                        except OSError:
                            pass
                    cands.append((e, full))
            except OSError:
                pass
        for name, full in cands:
            extra = ""
            if not quick:
                try:
                    total, _used, free = shutil.disk_usage(full)
                    used = total - free
                    percent = (used / total * 100) if total else 0
                    extra = (f"{human_size(used)} used of {human_size(total)} "
                             f"({percent:.1f}%)")
                except OSError:
                    pass
            items.append(Item(name, full, True, 0, None, False, extra))
    return items


def known_folders():
    home = os.path.expanduser("~")
    reg = {}
    if IS_WIN:
        try:
            import winreg
            ids = {"Desktop": "Desktop",
                   "Downloads": "{374DE290-123F-4565-9164-39C4925E467B}",
                   "Documents": "Personal",
                   "Pictures": "My Pictures",
                   "Music": "My Music",
                   "Videos": "My Video"}
            key = winreg.OpenKey(
                winreg.HKEY_CURRENT_USER,
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders")
            try:
                for label, val in ids.items():
                    try:
                        v, _t = winreg.QueryValueEx(key, val)
                        reg[label] = os.path.expandvars(v)
                    except OSError:
                        pass
            finally:
                winreg.CloseKey(key)
        except Exception:
            pass
    else:
        reg = _xdg_user_dirs()  # #11
    out = [("Home", home)]
    for label in ("Desktop", "Downloads", "Documents", "Pictures",
                  "Music", "Videos"):
        path = reg.get(label) or os.path.join(home, label)
        if not os.path.isdir(path):
            path = os.path.join(home, label)
        if os.path.isdir(path):
            out.append((label, path))
    # RECYCLE is a virtual path handled by scan_trash() (works on Windows,
    # macOS, Linux). Bug #29 is a false positive.
    out.append(("Recycle Bin", RECYCLE))
    return out