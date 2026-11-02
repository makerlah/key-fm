"""Drive discovery and known-folder resolution."""
import os
import shutil
import sys

from .constants import IS_WIN, RECYCLE
from .helpers import human_size
from .models import Item


def win_drive_roots(kinds=(2, 3)):
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


def scan_drives(quick=False):
    items = []
    if IS_WIN:
        import ctypes
        k32 = ctypes.windll.kernel32
        mask = k32.GetLogicalDrives()
        defaults = {2: "Removable Disk", 3: "Local Disk", 4: "Network Drive",
                    5: "CD Drive"}
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
        user = os.path.basename(os.path.expanduser("~"))
        bases = ["/Volumes"] if sys.platform == "darwin" else \
            [f"/media/{user}", f"/run/media/{user}", "/mnt"]
        for b in bases:
            try:
                for e in sorted(os.listdir(b)):
                    full = os.path.join(b, e)
                    if os.path.isdir(full):
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
    out = [("Home", home)]
    for label in ("Desktop", "Downloads", "Documents", "Pictures",
                  "Music", "Videos"):
        path = reg.get(label) or os.path.join(home, label)
        if not os.path.isdir(path):
            path = os.path.join(home, label)
        if os.path.isdir(path):
            out.append((label, path))
    out.append(("Recycle Bin", RECYCLE))
    return out