#!/usr/bin/env python3
"""
keyfm - a keyboard-driven file manager with tabs, dark mode, a sidebar with
Quick access, a working Recycle Bin and right-click menus.
(Python 3.8+, tkinter only - nothing to install)

Run:   python keyfm.py [start_directory]
Press F1 inside the app for every keyboard shortcut.

Optional: `pip install send2trash` lets Delete use the trash on Linux/macOS.
On Windows, Delete uses the real Recycle Bin without extra packages.
"""

import json
import ntpath
import os
import re
import shutil
import stat
import struct
import subprocess
import sys
import threading
import time
import tkinter as tk
from datetime import datetime
from tkinter import ttk, messagebox, simpledialog
from urllib.parse import quote, unquote

try:
    from send2trash import send2trash
except Exception:  # not installed
    send2trash = None

APP = "keyfm"
IS_WIN = sys.platform.startswith("win")
ALT_MASK = 0x20000 if IS_WIN else 0x8  # modifier bit used to ignore Alt+key in type-ahead

THIS_PC = "::this-pc::"        # virtual location: all drives
RECYCLE = "::recycle-bin::"    # virtual location: Recycle Bin / trash
VIRTUAL = (THIS_PC, RECYCLE)

COLS = ("name", "where", "size", "modified")
TITLES = {"name": "Name", "where": "Original location", "size": "Size",
          "modified": "Modified"}
PC_TITLES = {"name": "Name", "where": "", "size": "Free space", "modified": ""}
BIN_TITLES = {"name": "Name", "where": "Original location", "size": "Size",
              "modified": "Date deleted"}

CONFIG_FILE = os.path.join(os.path.expanduser("~"), ".keyfm.json")
OLD_PIN_FILE = os.path.join(os.path.expanduser("~"), ".keyfm_pins.json")

LIGHT = dict(bg="#f0f0f0", fg="#1a1a1a", field="#ffffff", side="#fafafa",
             sel_bg="#cce8ff", sel_fg="#000000", head="#f0f0f0",
             border="#d0d0d0", entry="#ffffff", tab="#e2e2e2",
             tab_sel="#ffffff", menu_bg="#f9f9f9", menu_active="#cce8ff",
             dim="#8a8a8a", scroll="#c2c2c2")
DARK = dict(bg="#202020", fg="#e8e8e8", field="#191919", side="#202020",
            sel_bg="#2f4f6f", sel_fg="#ffffff", head="#262626",
            border="#3a3a3a", entry="#2b2b2b", tab="#2b2b2b",
            tab_sel="#191919", menu_bg="#2b2b2b", menu_active="#3d3d3d",
            dim="#808080", scroll="#4a4a4a")

SHORTCUTS = [
    ("Tabs", [
        ("Ctrl+T", "New tab (opens at the current folder)"),
        ("Ctrl+W", "Close tab"),
        ("Ctrl+Tab / Ctrl+Shift+Tab", "Next / previous tab"),
        ("Ctrl+PgDn / Ctrl+PgUp", "Next / previous tab"),
        ("Alt+1 ... Alt+9", "Go to tab number 1-9"),
    ]),
    ("Moving around", [
        ("Up / Down", "Move the selection"),
        ("Home / End", "First / last item"),
        ("PgUp / PgDn", "Move 15 rows"),
        ("Enter / Right", "Open folder or file (Right: folders only)"),
        ("Backspace / Left / Alt+Up", "Go up one folder"),
        ("Alt+Left / Alt+Right", "Back / forward in history"),
        ("Alt+Home", "Go to your home folder"),
        ("Ctrl+Shift+M", "This PC (all drives)"),
        ("Ctrl+Shift+B", "Recycle Bin"),
        ("Ctrl+L", "Edit the path (Enter = go, Esc = cancel)"),
        ("Ctrl+F", "Filter the current folder (Esc = clear)"),
        ("<type letters>", "Jump to the item that starts with them"),
    ]),
    ("Selecting", [
        ("Shift+Up / Shift+Down", "Extend the selection"),
        ("Ctrl+A", "Select all"),
        ("Ctrl+Up / Ctrl+Down", "Move focus without changing the selection"),
        ("Space", "Toggle the focused item"),
    ]),
    ("Files", [
        ("Ctrl+C / Ctrl+X / Ctrl+V", "Copy / cut / paste"),
        ("Ctrl+Shift+C", "Copy the path of the selected items"),
        ("F2", "Rename"),
        ("Delete", "Move to the Recycle Bin"),
        ("Shift+Delete", "Delete permanently (asks first)"),
        ("Ctrl+N / Ctrl+Shift+N", "New file / new folder"),
        ("Alt+Enter", "Properties"),
        ("Right-click > Open with", "Choose an app for the selected file"),
        ("Menu key / Shift+F10", "Right-click menu"),
    ]),
    ("Recycle Bin", [
        ("Enter / Ctrl+Z", "Restore the selected items"),
        ("Delete", "Delete the selected items permanently"),
        ("Ctrl+Shift+Delete", "Empty the Recycle Bin"),
    ]),
    ("Sidebar and Quick access", [
        ("Ctrl+B", "Jump to the sidebar and back"),
        ("Enter (in sidebar)", "Open the highlighted entry"),
        ("Esc (in sidebar)", "Back to the file list"),
        ("Ctrl+P", "Pin the current folder to Quick access"),
        ("Ctrl+Shift+P", "Unpin the current folder"),
        ("Delete (in sidebar)", "Unpin the highlighted entry"),
    ]),
    ("View", [
        ("Ctrl+H", "Show / hide hidden files"),
        ("Ctrl+Shift+D", "Dark / light mode"),
        ("Ctrl+1 / 2 / 3", "Sort by name / size / modified (again = reverse)"),
        ("F5 / Ctrl+R", "Refresh"),
    ]),
    ("Program", [
        ("F10 or Alt+F/E/V/G/H", "Open the menus"),
        ("F1", "This list"),
        ("Ctrl+Q", "Quit"),
    ]),
]


class Item:
    __slots__ = ("name", "path", "is_dir", "size", "mtime", "hidden",
                 "extra", "orig", "info")

    def __init__(self, name, path, is_dir, size, mtime, hidden,
                 extra="", orig="", info=""):
        self.name = name
        self.path = path
        self.is_dir = is_dir
        self.size = size
        self.mtime = mtime
        self.hidden = hidden
        self.extra = extra   # drive free-space text
        self.orig = orig     # Recycle Bin: original full path
        self.info = info     # Recycle Bin: metadata file ($I... / .trashinfo)


class Tab:
    __slots__ = ("cwd", "back", "fwd", "sort_col", "sort_reverse",
                 "filter", "sel", "focus")

    def __init__(self, cwd=None):
        self.cwd = cwd
        self.back = []
        self.fwd = []
        self.sort_col = "name"
        self.sort_reverse = False
        self.filter = ""
        self.sel = []
        self.focus = None


# ---------------------------------------------------------------- helpers
def human_size(n):
    if n < 1024:
        return f"{n} B"
    n = float(n)
    for unit in ("KB", "MB", "GB", "TB"):
        n /= 1024.0
        if n < 1024 or unit == "TB":
            return f"{n:.1f} {unit}"


def fmt_time(ts):
    try:
        return datetime.fromtimestamp(ts).strftime("%Y-%m-%d %H:%M")
    except (OverflowError, OSError, ValueError, TypeError):
        return ""


def norm(p):
    return os.path.normcase(os.path.abspath(p))


def same_loc(a, b):
    if a in VIRTUAL or b in VIRTUAL:
        return a == b
    return norm(a) == norm(b)


def exists_loc(p):
    return p in VIRTUAL or os.path.isdir(p)


def qkey(path):
    return path if path in VIRTUAL else norm(path)


def loc_dirname(p):
    return ntpath.dirname(p) if IS_WIN else os.path.dirname(p)


def loc_basename(p):
    return ntpath.basename(p) if IS_WIN else os.path.basename(p)


def is_inside(child, parent):
    """True if `child` is `parent` or located somewhere below it."""
    try:
        return os.path.commonpath([norm(child), norm(parent)]) == norm(parent)
    except ValueError:  # different drives
        return False


def unique_path(folder, name, is_dir):
    """Return a non-existing path in `folder`, adding ' - copy' if needed."""
    dest = os.path.join(folder, name)
    if not os.path.lexists(dest):
        return dest
    base, ext = (name, "") if is_dir else os.path.splitext(name)
    n = 1
    while True:
        suffix = " - copy" if n == 1 else f" - copy {n}"
        dest = os.path.join(folder, f"{base}{suffix}{ext}")
        if not os.path.lexists(dest):
            return dest
        n += 1


def _force_remove(func, path, *_):
    """rmtree error handler: clear read-only flag and retry (Windows)."""
    os.chmod(path, stat.S_IWRITE)
    func(path)


def remove_path(path):
    if os.path.islink(path) or not os.path.isdir(path):
        try:
            os.remove(path)
        except OSError:
            try:
                os.chmod(path, stat.S_IWRITE)
                os.remove(path)
            except OSError:
                os.rmdir(path)  # directory symlink / junction on Windows
    else:
        if sys.version_info >= (3, 12):
            shutil.rmtree(path, onexc=_force_remove)
        else:
            shutil.rmtree(path, onerror=_force_remove)


def dir_stats(path, cancel):
    """(files, folders, bytes) below `path`; stops early if cancel is set."""
    files = folders = size = 0
    stack = [path]
    while stack:
        if cancel.is_set():
            break
        p = stack.pop()
        try:
            with os.scandir(p) as it:
                for e in it:
                    try:
                        if e.is_dir(follow_symlinks=False):
                            folders += 1
                            stack.append(e.path)
                        else:
                            files += 1
                            size += e.stat(follow_symlinks=False).st_size
                    except OSError:
                        pass
        except OSError:
            pass
    return files, folders, size


def brk(fn):
    """Wrap a method as a Tk event handler that stops further handling."""
    def handler(event=None):
        fn()
        return "break"
    return handler


# ---------------------------------------------------------- drives / places
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
    """List drives / mount points as Items. quick=True skips slow lookups."""
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
            if not (quick and dtype == 4):  # don't poke network drives at startup
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
                    extra = f"{human_size(used)} used of {human_size(total)} ({percent:.1f}%)"
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
                    extra = f"{human_size(used)} used of {human_size(total)} ({percent:.1f}%)"
                except OSError:
                    pass
            items.append(Item(name, full, True, 0, None, False, extra))
    return items


def known_folders():
    """[(label, path)] for Home, Desktop, Downloads, ... and the Recycle Bin."""
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
    for label in ("Desktop", "Downloads", "Documents", "Pictures", "Music", "Videos"):
        path = reg.get(label) or os.path.join(home, label)
        if not os.path.isdir(path):
            path = os.path.join(home, label)
        if os.path.isdir(path):
            out.append((label, path))
    out.append(("Recycle Bin", RECYCLE))
    return out


# ------------------------------------------------------------------ config
def load_config():
    cfg = {"dark": True, "show_hidden": False, "pins": [], "unpinned": []}
    data = None
    try:
        with open(CONFIG_FILE, encoding="utf-8") as f:
            data = json.load(f)
    except Exception:
        try:  # migrate pins from the older version
            with open(OLD_PIN_FILE, encoding="utf-8") as f:
                old = json.load(f)
            if isinstance(old, list):
                data = {"pins": old}
        except Exception:
            data = None
    if isinstance(data, dict):
        for k in ("dark", "show_hidden"):
            if isinstance(data.get(k), bool):
                cfg[k] = data[k]
        for k in ("pins", "unpinned"):
            if isinstance(data.get(k), list):
                cfg[k] = [p for p in data[k] if isinstance(p, str)]
    return cfg


def save_config(cfg):
    try:
        with open(CONFIG_FILE, "w", encoding="utf-8") as f:
            json.dump(cfg, f, indent=2)
        return True
    except OSError:
        return False


# ------------------------------------------------------------ Recycle Bin
_SID = None


def win_sid():
    """Current user's SID string (needed to find $Recycle.Bin\\<SID>)."""
    global _SID
    if _SID is None:
        _SID = ""
        try:
            out = subprocess.run(
                ["whoami", "/user", "/fo", "csv", "/nh"],
                capture_output=True, text=True, timeout=10,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)).stdout
            m = re.search(r"S-1-\d+(?:-\d+)+", out)
            if m:
                _SID = m.group(0)
        except Exception:
            pass
    return _SID


def parse_i_file(data):
    """Parse a Windows $I file -> (size, unix_time_or_None, original_path)."""
    if len(data) < 24:
        return None
    ver, size, ft = struct.unpack_from("<qqQ", data, 0)
    if ver == 2:
        if len(data) < 28:
            return None
        n = struct.unpack_from("<I", data, 24)[0]
        raw = data[28:28 + n * 2]
    elif ver == 1:
        raw = data[24:24 + 520]
    else:
        return None
    path = raw.decode("utf-16-le", "ignore").split("\x00")[0]
    ts = (ft / 10_000_000 - 11644473600) if ft else None
    return size, ts, path


def scan_bin_dir(bin_dir):
    """Items for one Windows $Recycle.Bin\\<SID> folder."""
    items = []
    try:
        names = os.listdir(bin_dir)
    except OSError:
        return items
    for n in names:
        if not n.upper().startswith("$I"):
            continue
        ipath = os.path.join(bin_dir, n)
        rpath = os.path.join(bin_dir, "$R" + n[2:])
        if not os.path.lexists(rpath):
            continue
        try:
            with open(ipath, "rb") as f:
                parsed = parse_i_file(f.read())
        except OSError:
            parsed = None
        if not parsed:
            continue
        size, ts, orig = parsed
        name = ntpath.basename(orig) or n
        is_dir = os.path.isdir(rpath) and not os.path.islink(rpath)
        items.append(Item(name, rpath, is_dir, size, ts, False, "", orig, ipath))
    return items


def scan_trash():
    """Everything in the Recycle Bin (Windows) / trash (Linux, macOS)."""
    items = []
    try:
        if IS_WIN:
            sid = win_sid()
            for root in win_drive_roots():
                base = os.path.join(root, "$Recycle.Bin")
                if sid:
                    dirs = [os.path.join(base, sid)]
                else:
                    try:
                        dirs = [os.path.join(base, d) for d in os.listdir(base)]
                    except OSError:
                        dirs = []
                for d in dirs:
                    items.extend(scan_bin_dir(d))
        elif sys.platform == "darwin":
            tdir = os.path.join(os.path.expanduser("~"), ".Trash")
            for n in sorted(os.listdir(tdir)):
                rp = os.path.join(tdir, n)
                try:
                    st = os.lstat(rp)
                except OSError:
                    continue
                items.append(Item(n, rp, os.path.isdir(rp) and not os.path.islink(rp),
                                  st.st_size, st.st_mtime, False))
        else:
            data_home = os.environ.get("XDG_DATA_HOME") or os.path.join(
                os.path.expanduser("~"), ".local", "share")
            tdir = os.path.join(data_home, "Trash")
            files, infos = os.path.join(tdir, "files"), os.path.join(tdir, "info")
            for n in sorted(os.listdir(files)):
                rp = os.path.join(files, n)
                ip = os.path.join(infos, n + ".trashinfo")
                orig, ts = "", None
                try:
                    with open(ip, encoding="utf-8", errors="replace") as f:
                        for line in f:
                            if line.startswith("Path="):
                                orig = unquote(line[5:].strip())
                            elif line.startswith("DeletionDate="):
                                try:
                                    ts = datetime.strptime(
                                        line[13:].strip()[:19],
                                        "%Y-%m-%dT%H:%M:%S").timestamp()
                                except ValueError:
                                    pass
                except OSError:
                    pass
                try:
                    st = os.lstat(rp)
                except OSError:
                    continue
                name = os.path.basename(orig) or n
                items.append(Item(name, rp, os.path.isdir(rp) and not os.path.islink(rp),
                                  st.st_size, ts or st.st_mtime, False, "", orig,
                                  ip if os.path.exists(ip) else ""))
    except OSError:
        pass
    return items


def win_recycle(path):
    """Move one path to the Windows Recycle Bin (SHFileOperationW)."""
    import ctypes
    from ctypes import wintypes
    path = os.path.abspath(path)

    class SHFILEOPSTRUCTW(ctypes.Structure):
        _pack_ = 1 if ctypes.sizeof(ctypes.c_void_p) == 4 else 8
        _fields_ = [("hwnd", wintypes.HWND),
                    ("wFunc", wintypes.UINT),
                    ("pFrom", wintypes.LPCWSTR),
                    ("pTo", wintypes.LPCWSTR),
                    ("fFlags", ctypes.c_ushort),
                    ("fAnyOperationsAborted", ctypes.BOOL),
                    ("hNameMappings", ctypes.c_void_p),
                    ("lpszProgressTitle", wintypes.LPCWSTR)]

    buf = ctypes.create_unicode_buffer(path, len(path) + 2)  # double-NUL terminated
    op = SHFILEOPSTRUCTW()
    op.hwnd = None
    op.wFunc = 3                 # FO_DELETE
    op.pFrom = wintypes.LPCWSTR(ctypes.addressof(buf))
    op.pTo = None
    op.fFlags = 0x0040 | 0x0400  # FOF_ALLOWUNDO | FOF_NOERRORUI
    res = ctypes.windll.shell32.SHFileOperationW(ctypes.byref(op))
    if op.fAnyOperationsAborted:
        raise OSError("cancelled")
    if res != 0 or os.path.lexists(path):
        raise OSError(f"could not move to the Recycle Bin (code {res})")


def move_to_trash(path):
    """Move a path to the platform trash, with an XDG fallback on Linux."""
    if IS_WIN:
        win_recycle(path)
    elif send2trash is not None:
        send2trash(path)
    elif sys.platform.startswith("linux"):
        data_home = os.environ.get("XDG_DATA_HOME") or os.path.join(
            os.path.expanduser("~"), ".local", "share")
        trash_root = os.path.join(data_home, "Trash")
        files_dir = os.path.join(trash_root, "files")
        info_dir = os.path.join(trash_root, "info")
        os.makedirs(files_dir, mode=0o700, exist_ok=True)
        os.makedirs(info_dir, mode=0o700, exist_ok=True)
        name = os.path.basename(path.rstrip(os.sep)) or "item"
        dest = unique_path(files_dir, name, os.path.isdir(path))
        info_path = os.path.join(info_dir, os.path.basename(dest) + ".trashinfo")
        with open(info_path, "w", encoding="utf-8") as f:
            f.write("[Trash Info]\\n")
            f.write("Path=" + quote(os.path.abspath(path), safe="/") + "\\n")
            f.write("DeletionDate=" + datetime.now().strftime("%Y-%m-%dT%H:%M:%S") + "\\n")
        try:
            shutil.move(path, dest)
        except Exception:
            try:
                os.remove(info_path)
            except OSError:
                pass
            raise
    else:
        raise OSError("Trash support unavailable; install send2trash")


def menu_add(menu, label, command, accel="", state="normal"):
    menu.add_command(label=label, command=command, accelerator=accel, state=state)


class FileManager:
    def __init__(self, root, start):
        self.root = root
        self.cfg = load_config()
        self.dark = self.cfg["dark"]
        self.show_hidden = self.cfg["show_hidden"]
        self.palette = DARK if self.dark else LIGHT
        self.cwd = None
        self.entries = []      # everything in the current location
        self.shown = []        # filtered + sorted (tree iid == index into this list)
        self.back = []
        self.fwd = []
        self.sort_col = "name"
        self.sort_reverse = False
        self.clip = []
        self.clip_mode = "copy"
        self.msg = ""
        self._flash_job = None
        self._suppress_filter = False
        self._ta_buf = ""
        self._ta_time = 0.0
        self.side_paths = {}
        self.menus = []
        self.menubuttons = []
        self.menu_keys = []
        self.quick = known_folders()
        self.tabs = [Tab()]
        self.cur = 0

        self.build_ui()
        self.build_menus()
        self.apply_theme()
        self.bind_keys()
        self.build_sidebar()
        home = os.path.expanduser("~")
        if not self.navigate(start, record=False):
            self.navigate(home, record=False)
        self.tree.focus_set()

    # ------------------------------------------------------------------ UI
    def build_ui(self):
        r = self.root
        r.title(APP)
        r.geometry("1020x660")
        r.minsize(620, 360)

        self.menubar = tk.Frame(r)
        self.menubar.pack(fill="x")

        self.notebook = ttk.Notebook(r, takefocus=0)
        self.notebook.pack(fill="x", padx=6, pady=(4, 0))
        self.notebook.add(ttk.Frame(self.notebook, height=1), text="Home")

        top = ttk.Frame(r, padding=(6, 6, 6, 4))
        top.pack(fill="x")
        ttk.Label(top, text="Path:").pack(side="left")
        self.path_var = tk.StringVar()
        self.path_entry = ttk.Entry(top, textvariable=self.path_var)
        self.path_entry.pack(side="left", fill="x", expand=True, padx=(6, 0))

        mid = ttk.Frame(r)
        mid.pack(fill="both", expand=True, padx=6)
        self.sidebar = ttk.Treeview(mid, show="tree", selectmode="browse",
                                    style="Side.Treeview")
        self.sidebar.column("#0", width=210, stretch=False)
        self.sidebar.pack(side="left", fill="y", padx=(0, 6))

        self.tree = ttk.Treeview(mid, columns=COLS, show="headings",
                                 selectmode="extended",
                                 displaycolumns=("name", "size", "modified"))
        vsb = ttk.Scrollbar(mid, orient="vertical", command=self.tree.yview)
        self.tree.configure(yscrollcommand=vsb.set)
        for c in COLS:
            self.tree.heading(c, text=TITLES[c],
                              command=lambda col=c: self.set_sort(col))
        self.tree.column("name", width=460, anchor="w", stretch=True)
        self.tree.column("where", width=260, anchor="w", stretch=False)
        self.tree.column("size", width=100, anchor="e", stretch=False)
        self.tree.column("modified", width=160, anchor="w", stretch=False)
        self.tree.pack(side="left", fill="both", expand=True)
        vsb.pack(side="right", fill="y")

        bar = ttk.Frame(r, padding=(6, 4, 6, 0))
        bar.pack(fill="x")
        ttk.Label(bar, text="Filter (Ctrl+F):").pack(side="left")
        self.filter_var = tk.StringVar()
        self.filter_entry = ttk.Entry(bar, textvariable=self.filter_var)
        self.filter_entry.pack(side="left", fill="x", expand=True, padx=(6, 0))

        self.status_var = tk.StringVar()
        ttk.Label(r, textvariable=self.status_var, anchor="w",
                  padding=(8, 4)).pack(fill="x")

        self.ctx = tk.Menu(r, tearoff=0)   # reusable right-click menu
        self.update_headings()

    def build_menus(self):
        self.hidden_var = tk.BooleanVar(master=self.root, value=self.show_hidden)
        self.dark_var = tk.BooleanVar(master=self.root, value=self.dark)

        def top(label, underline, key):
            mb = tk.Menubutton(self.menubar, text=label, underline=underline,
                               padx=10, pady=3, relief="flat", bd=0,
                               highlightthickness=0, takefocus=0)
            m = tk.Menu(mb, tearoff=0)
            mb.configure(menu=m)
            mb.pack(side="left")
            self.menubuttons.append(mb)
            self.menus.append(m)
            self.menu_keys.append((key, mb))
            return m

        f = top("File", 0, "f")
        menu_add(f, "New tab", self.new_tab, "Ctrl+T")
        menu_add(f, "Close tab", self.close_tab, "Ctrl+W")
        menu_add(f, "Next tab", lambda: self.next_tab(1), "Ctrl+Tab")
        menu_add(f, "Previous tab", lambda: self.next_tab(-1), "Ctrl+Shift+Tab")
        f.add_separator()
        menu_add(f, "New file", lambda: self.new_entry(False), "Ctrl+N")
        menu_add(f, "New folder", lambda: self.new_entry(True), "Ctrl+Shift+N")
        menu_add(f, "Properties", self.properties_sel, "Alt+Enter")
        f.add_separator()
        menu_add(f, "Quit", self.root.destroy, "Ctrl+Q")

        e = top("Edit", 0, "e")
        menu_add(e, "Cut", lambda: self.copy_sel("cut"), "Ctrl+X")
        menu_add(e, "Copy", lambda: self.copy_sel("copy"), "Ctrl+C")
        menu_add(e, "Paste", self.paste, "Ctrl+V")
        menu_add(e, "Copy path", self.copy_path, "Ctrl+Shift+C")
        e.add_separator()
        menu_add(e, "Select all", self.select_all, "Ctrl+A")
        menu_add(e, "Rename", self.rename, "F2")
        e.add_separator()
        menu_add(e, "Delete (to Recycle Bin)", self.delete_sel, "Delete")
        menu_add(e, "Delete permanently", lambda: self.delete_sel(permanent=True),
                 "Shift+Delete")
        menu_add(e, "Restore from Recycle Bin", self.restore_selected, "Enter / Ctrl+Z")
        menu_add(e, "Empty Recycle Bin", self.empty_bin, "Ctrl+Shift+Delete")

        v = top("View", 0, "v")
        v.add_checkbutton(label="Show hidden files", variable=self.hidden_var,
                          command=self.toggle_hidden, accelerator="Ctrl+H")
        v.add_checkbutton(label="Dark mode", variable=self.dark_var,
                          command=self.toggle_dark, accelerator="Ctrl+Shift+D")
        v.add_separator()
        menu_add(v, "Sort by name", lambda: self.set_sort("name"), "Ctrl+1")
        menu_add(v, "Sort by size", lambda: self.set_sort("size"), "Ctrl+2")
        menu_add(v, "Sort by date", lambda: self.set_sort("modified"), "Ctrl+3")
        v.add_separator()
        menu_add(v, "Refresh", self.reload, "F5")

        g = top("Go", 0, "g")
        menu_add(g, "Back", self.go_back, "Alt+Left")
        menu_add(g, "Forward", self.go_forward, "Alt+Right")
        menu_add(g, "Up", self.go_parent, "Backspace")
        menu_add(g, "Home folder", lambda: self.navigate(os.path.expanduser("~")),
                 "Alt+Home")
        menu_add(g, "This PC", lambda: self.navigate(THIS_PC), "Ctrl+Shift+M")
        menu_add(g, "Recycle Bin", lambda: self.navigate(RECYCLE), "Ctrl+Shift+B")
        g.add_separator()
        menu_add(g, "Edit path", self.focus_path, "Ctrl+L")
        menu_add(g, "Filter", self.focus_filter, "Ctrl+F")
        menu_add(g, "Sidebar", self.toggle_sidebar_focus, "Ctrl+B")
        g.add_separator()
        menu_add(g, "Pin current folder to Quick access", self.pin_current, "Ctrl+P")
        menu_add(g, "Unpin current folder", self.unpin_current, "Ctrl+Shift+P")

        h = top("Help", 0, "h")
        menu_add(h, "Keyboard shortcuts", self.show_help, "F1")

    # --------------------------------------------------------------- theme
    def style_menu(self, m):
        P = self.palette
        m.configure(bg=P["menu_bg"], fg=P["fg"], activebackground=P["menu_active"],
                    activeforeground=P["fg"], disabledforeground=P["dim"],
                    selectcolor=P["fg"], bd=1, relief="flat")

    def new_menu(self, parent):
        m = tk.Menu(parent, tearoff=0)
        self.style_menu(m)
        return m

    def apply_theme(self):
        P = DARK if self.dark else LIGHT
        self.palette = P
        st = ttk.Style(self.root)
        try:
            st.theme_use("clam")
        except tk.TclError:
            pass
        st.configure(".", background=P["bg"], foreground=P["fg"],
                     fieldbackground=P["entry"], bordercolor=P["border"],
                     lightcolor=P["bg"], darkcolor=P["bg"], troughcolor=P["bg"],
                     focuscolor=P["bg"], insertcolor=P["fg"],
                     selectbackground=P["sel_bg"], selectforeground=P["sel_fg"])
        st.configure("TFrame", background=P["bg"])
        st.configure("TLabel", background=P["bg"], foreground=P["fg"])
        st.configure("TEntry", fieldbackground=P["entry"], foreground=P["fg"],
                     insertcolor=P["fg"], bordercolor=P["border"],
                     lightcolor=P["entry"], darkcolor=P["entry"])
        st.configure("TButton", background=P["tab"], foreground=P["fg"],
                     bordercolor=P["border"], lightcolor=P["tab"],
                     darkcolor=P["tab"], padding=(14, 4))
        st.map("TButton", background=[("active", P["menu_active"]),
                                      ("pressed", P["sel_bg"])],
               foreground=[("active", P["fg"])])
        for name, bgc in (("Treeview", P["field"]), ("Side.Treeview", P["side"])):
            st.configure(name, background=bgc, fieldbackground=bgc,
                         foreground=P["fg"], rowheight=24, bordercolor=bgc,
                         lightcolor=bgc, darkcolor=bgc, borderwidth=0)
            st.map(name, background=[("selected", P["sel_bg"])],
                   foreground=[("selected", P["sel_fg"])])
        st.configure("Treeview.Heading", background=P["head"], foreground=P["fg"],
                     relief="flat", bordercolor=P["border"], lightcolor=P["head"],
                     darkcolor=P["head"], padding=(6, 4))
        st.map("Treeview.Heading", background=[("active", P["menu_active"])])
        st.configure("TScrollbar", background=P["scroll"], troughcolor=P["bg"],
                     bordercolor=P["bg"], arrowcolor=P["fg"],
                     lightcolor=P["scroll"], darkcolor=P["scroll"])
        st.map("TScrollbar", background=[("active", P["dim"])])
        st.configure("TNotebook", background=P["bg"], borderwidth=0,
                     bordercolor=P["bg"], lightcolor=P["bg"], darkcolor=P["bg"])
        st.configure("TNotebook.Tab", background=P["tab"], foreground=P["fg"],
                     padding=(14, 5), bordercolor=P["border"],
                     lightcolor=P["tab"], darkcolor=P["tab"])
        st.map("TNotebook.Tab",
               background=[("selected", P["tab_sel"]), ("active", P["menu_active"])],
               foreground=[("selected", P["fg"])])
        self.root.configure(background=P["bg"])
        self.menubar.configure(bg=P["bg"])
        for mb in self.menubuttons:
            mb.configure(bg=P["bg"], fg=P["fg"], activebackground=P["menu_active"],
                         activeforeground=P["fg"])
        for m in self.menus + [self.ctx]:
            self.style_menu(m)

    def toggle_dark(self):
        self.dark = not self.dark
        self.dark_var.set(self.dark)
        self.apply_theme()
        self.cfg["dark"] = self.dark
        save_config(self.cfg)
        self.flash("Dark mode on" if self.dark else "Light mode on")

    # ---------------------------------------------------------------- keys
    def gbind(self, seq, fn):
        """Bind on the window AND the list/sidebar (widget bindings would
        otherwise win, e.g. <Left> beating <Alt-Left>)."""
        h = brk(fn)
        for w in (self.root, self.tree, self.sidebar):
            w.bind(seq, h)

    def gctrl(self, letter, fn, shift=False):
        mod = "Control-Shift-" if shift else "Control-"
        for ch in (letter.lower(), letter.upper()):
            self.gbind(f"<{mod}{ch}>", fn)

    @staticmethod
    def bind_ctrl(widget, letter, fn):
        h = brk(fn)
        widget.bind(f"<Control-{letter.lower()}>", h)
        widget.bind(f"<Control-{letter.upper()}>", h)

    def bind_keys(self):
        r, t, sb = self.root, self.tree, self.sidebar

        # ---- file-list keys
        t.bind("<Return>", brk(self.open_focused))
        t.bind("<KP_Enter>", brk(self.open_focused))
        t.bind("<Alt-Return>", brk(self.properties_sel))
        t.bind("<Right>", brk(lambda: self.open_focused(dirs_only=True)))
        t.bind("<BackSpace>", brk(self.go_parent))
        t.bind("<Left>", brk(self.go_parent))
        t.bind("<Delete>", brk(self.delete_sel))
        t.bind("<Shift-Delete>", brk(lambda: self.delete_sel(permanent=True)))
        t.bind("<F2>", brk(self.rename))
        t.bind("<Home>", brk(lambda: self.jump(0)))
        t.bind("<End>", brk(lambda: self.jump(len(self.shown) - 1)))
        t.bind("<Prior>", brk(lambda: self.move(-15)))
        t.bind("<Next>", brk(lambda: self.move(15)))
        t.bind("<Control-Up>", brk(lambda: self.move(-1, select=False)))
        t.bind("<Control-Down>", brk(lambda: self.move(1, select=False)))
        t.bind("<Escape>", brk(self.on_escape))
        t.bind("<Double-1>", self.on_double)
        t.bind("<Key>", self.on_typeahead)
        t.bind("<<TreeviewSelect>>", lambda e: self.update_status())
        t.bind("<Button-3>", self.on_tree_context)
        if sys.platform == "darwin":
            t.bind("<Button-2>", self.on_tree_context)
            t.bind("<Control-Button-1>", self.on_tree_context)
        for seq in ("<App>", "<Menu>", "<Shift-F10>"):
            try:
                t.bind(seq, brk(self.context_from_keyboard))
            except tk.TclError:
                pass

        self.bind_ctrl(t, "a", self.select_all)
        self.bind_ctrl(t, "c", lambda: self.copy_sel("copy"))
        self.bind_ctrl(t, "x", lambda: self.copy_sel("cut"))
        self.bind_ctrl(t, "v", self.paste)
        self.bind_ctrl(t, "n", lambda: self.new_entry(False))
        self.bind_ctrl(t, "z", self.restore_selected)
        for ch in ("n", "N"):
            t.bind(f"<Control-Shift-{ch}>", brk(lambda: self.new_entry(True)))
        for ch in ("c", "C"):
            t.bind(f"<Control-Shift-{ch}>", brk(self.copy_path))

        # ---- global keys (window, list and sidebar)
        for ch in ("h", "H"):  # Ctrl+H must not eat Backspace-in-entry
            for w in (r, t, sb):
                w.bind(f"<Control-{ch}>", self.on_ctrl_h)
        self.gctrl("l", self.focus_path)
        self.gctrl("f", self.focus_filter)
        self.gctrl("r", self.reload)
        self.gctrl("q", self.root.destroy)
        self.gctrl("b", self.toggle_sidebar_focus)
        self.gctrl("p", self.pin_current)
        self.gctrl("p", self.unpin_current, shift=True)
        self.gctrl("m", lambda: self.navigate(THIS_PC), shift=True)
        self.gctrl("b", lambda: self.navigate(RECYCLE), shift=True)
        self.gctrl("d", self.toggle_dark, shift=True)
        self.gctrl("t", self.new_tab)
        self.gctrl("w", self.close_tab)
        self.gbind("<F5>", self.reload)
        self.gbind("<F1>", self.show_help)
        self.gbind("<F10>", lambda: self.post_menu(self.menubuttons[0]))
        self.gbind("<Alt-Left>", self.go_back)
        self.gbind("<Alt-Right>", self.go_forward)
        self.gbind("<Alt-Up>", self.go_parent)
        self.gbind("<Alt-Home>", lambda: self.navigate(os.path.expanduser("~")))
        self.gbind("<Control-Key-1>", lambda: self.set_sort("name"))
        self.gbind("<Control-Key-2>", lambda: self.set_sort("size"))
        self.gbind("<Control-Key-3>", lambda: self.set_sort("modified"))
        self.gbind("<Control-Shift-Delete>", self.empty_bin)
        self.gbind("<Control-Tab>", lambda: self.next_tab(1))
        self.gbind("<Control-Next>", lambda: self.next_tab(1))
        self.gbind("<Control-Prior>", lambda: self.next_tab(-1))
        for seq in ("<Control-Shift-Tab>", "<Control-ISO_Left_Tab>"):
            try:
                self.gbind(seq, lambda: self.next_tab(-1))
            except tk.TclError:
                pass
        for n in range(1, 10):
            self.gbind(f"<Alt-Key-{n}>", lambda n=n: self.goto_tab(n - 1))
        for key, mb in self.menu_keys:
            self.gbind(f"<Alt-{key}>", lambda mb=mb: self.post_menu(mb))

        # ---- tabs
        self.notebook.bind("<<NotebookTabChanged>>", self.on_tab_changed)
        self.notebook.bind("<ButtonRelease-1>",
                           lambda e: self.root.after_idle(self.tree.focus_set))
        self.notebook.bind("<Button-2>", self.on_tab_middle_click)

        # ---- sidebar
        sb.bind("<Return>", brk(self.side_activate))
        sb.bind("<KP_Enter>", brk(self.side_activate))
        sb.bind("<Escape>", brk(self.focus_tree))
        sb.bind("<Delete>", brk(self.unpin_side))
        sb.bind("<ButtonRelease-1>", self.on_side_click)
        sb.bind("<Button-3>", self.on_side_context)

        # ---- path entry
        self.path_entry.bind("<Return>", self.on_path_enter)
        self.path_entry.bind("<Escape>", self.on_path_escape)

        # ---- filter entry
        self.filter_var.trace_add("write", self.on_filter_change)
        self.filter_entry.bind("<Return>", lambda e: self.focus_tree())
        self.filter_entry.bind("<Down>", lambda e: self.focus_tree())
        self.filter_entry.bind("<Escape>", self.on_filter_escape)

    def on_ctrl_h(self, event):
        if isinstance(event.widget, ttk.Entry):
            return None  # in a text box Ctrl+H is Backspace
        self.toggle_hidden()
        return "break"

    def post_menu(self, mb):
        menu = mb.nametowidget(mb.cget("menu"))
        try:
            menu.tk_popup(mb.winfo_rootx(), mb.winfo_rooty() + mb.winfo_height())
        finally:
            menu.grab_release()

    # ------------------------------------------------------------- Navigation
    def update_headings(self):
        titles = PC_TITLES if self.cwd == THIS_PC else (
            BIN_TITLES if self.cwd == RECYCLE else TITLES
        )
        cols = ("name", "size", "modified") if self.cwd != RECYCLE else COLS
        self.tree.configure(displaycolumns=cols)
        for c in COLS:
            self.tree.heading(c, text=titles[c])

    def build_sidebar(self):
        sb = self.sidebar
        sb.delete(*sb.get_children())
        self.side_paths.clear()

        qa = sb.insert("", "end", "qa_head", text="Quick access", open=True)
        for name, p in self.quick:
            iid = sb.insert(qa, "end", text=name)
            self.side_paths[iid] = p

        pins = [p for p in self.cfg.get("pins", []) if os.path.isdir(p)]
        if pins:
            for p in pins:
                name = os.path.basename(p) or p
                iid = sb.insert(qa, "end", text=f"📌 {name}")
                self.side_paths[iid] = p

        pc = sb.insert("", "end", "pc_head", text="This PC", open=True)
        self.side_paths[pc] = THIS_PC
        for d in scan_drives(quick=True):
            iid = sb.insert(pc, "end", text=d.name)
            self.side_paths[iid] = d.path

    def navigate(self, target, record=True):
        if not exists_loc(target):
            self.flash(f"Path does not exist: {target}")
            return False

        if record and self.cwd and not same_loc(self.cwd, target):
            self.back.append(self.cwd)
            self.fwd.clear()

        self.cwd = target
        self.update_headings()
        self.reload()
        if self.tabs and 0 <= self.cur < len(self.tabs):
            tab = self.tabs[self.cur]
            tab.cwd = target
            tab.back = self.back
            tab.fwd = self.fwd
            tab.sort_col = self.sort_col
            tab.sort_reverse = self.sort_reverse
            tab.filter = self.filter_var.get() if hasattr(self, "filter_var") else ""

        # Update path bar text
        display_path = target
        if target == THIS_PC:
            display_path = "This PC"
        elif target == RECYCLE:
            display_path = "Recycle Bin"
        self.path_var.set(display_path)

        # Update tab title
        title = loc_basename(target) if target not in VIRTUAL else display_path
        if not title:
            title = target
        self.notebook.tab(self.cur, text=title[:18])
        return True

    def reload(self):
        self.entries.clear()
        if self.cwd == THIS_PC:
            self.entries = scan_drives(quick=False)
        elif self.cwd == RECYCLE:
            self.entries = scan_trash()
        elif os.path.isdir(self.cwd):
            try:
                with os.scandir(self.cwd) as it:
                    for e in it:
                        hidden = e.name.startswith(".")
                        if IS_WIN:
                            try:
                                st_flags = e.stat(follow_symlinks=False).st_file_attributes
                                if st_flags & stat.FILE_ATTRIBUTE_HIDDEN:
                                    hidden = True
                            except OSError:
                                pass
                        if hidden and not self.show_hidden:
                            continue
                        try:
                            st = e.stat(follow_symlinks=False)
                            is_dir = e.is_dir(follow_symlinks=False)
                            size = 0 if is_dir else st.st_size
                            mtime = st.st_mtime
                        except OSError:
                            is_dir, size, mtime = False, 0, 0
                        self.entries.append(Item(e.name, e.path, is_dir, size, mtime, hidden))
            except OSError as err:
                self.flash(f"Access error: {err}")

        self.apply_filter_and_sort()

    def apply_filter_and_sort(self):
        flt = self.filter_var.get().lower()
        res = [e for e in self.entries if flt in e.name.lower()]

        def sort_key(item):
            col = self.sort_col
            if col == "name":
                val = item.name.lower()
            elif col == "size":
                val = item.size
            elif col == "modified":
                val = item.mtime or 0
            elif col == "where":
                val = (item.orig or "").lower()
            else:
                val = item.name.lower()
            # Folders listed first
            return (not item.is_dir, val)

        res.sort(key=sort_key, reverse=self.sort_reverse)
        self.shown = res
        self.populate_tree()

    def populate_tree(self):
        self.tree.delete(*self.tree.get_children())
        for idx, item in enumerate(self.shown):
            sz = human_size(item.size) if not item.is_dir else ""
            if item.extra:
                sz = item.extra
            dt = fmt_time(item.mtime) if item.mtime else ""
            prefix = "📁 " if item.is_dir else "📄 "
            self.tree.insert("", "end", iid=str(idx),
                             values=(prefix + item.name, item.orig, sz, dt))

        if self.shown:
            self.tree.selection_set("0")
            self.tree.focus("0")
        self.update_status()

    def set_sort(self, col):
        if self.sort_col == col:
            self.sort_reverse = not self.sort_reverse
        else:
            self.sort_col = col
            self.sort_reverse = False
        self.apply_filter_and_sort()

    # ----------------------------------------------------------- Event Handlers
    def move(self, delta, select=True):
        if not self.shown:
            return
        curr = self.tree.focus()
        idx = int(curr) if curr.isdigit() else 0
        target = max(0, min(len(self.shown) - 1, idx + delta))
        iid = str(target)
        self.tree.focus(iid)
        if select:
            self.tree.selection_set(iid)
        self.tree.see(iid)

    def jump(self, index):
        if not self.shown:
            return
        target = max(0, min(len(self.shown) - 1, index))
        iid = str(target)
        self.tree.focus(iid)
        self.tree.selection_set(iid)
        self.tree.see(iid)

    def open_focused(self, dirs_only=False):
        curr = self.tree.focus()
        if not curr.isdigit():
            return
        item = self.shown[int(curr)]
        if item.is_dir:
            self.navigate(item.path)
        elif not dirs_only:
            if self.cwd == RECYCLE:
                self.restore_selected()
            else:
                try:
                    if IS_WIN:
                        os.startfile(item.path)
                    elif sys.platform == "darwin":
                        subprocess.Popen(["open", item.path])
                    else:
                        subprocess.Popen(["xdg-open", item.path])
                except Exception as err:
                    self.flash(f"Cannot open file: {err}")

    def on_double(self, event):
        row = self.tree.identify_row(event.y)
        if row:
            self.tree.focus(row)
            self.tree.selection_set(row)
            self.open_focused()

    def go_parent(self):
        if self.cwd in VIRTUAL:
            return
        parent = loc_dirname(self.cwd)
        if parent and not same_loc(parent, self.cwd):
            self.navigate(parent)

    def go_back(self):
        if self.back:
            self.fwd.append(self.cwd)
            dest = self.back.pop()
            self.navigate(dest, record=False)

    def go_forward(self):
        if self.fwd:
            self.back.append(self.cwd)
            dest = self.fwd.pop()
            self.navigate(dest, record=False)

    def toggle_hidden(self):
        self.show_hidden = not self.show_hidden
        self.hidden_var.set(self.show_hidden)
        self.cfg["show_hidden"] = self.show_hidden
        save_config(self.cfg)
        self.reload()

    # ---------------------------------------------------------- Path & Filtering
    def focus_path(self):
        self.path_entry.focus_set()
        self.path_entry.selection_range(0, "end")

    def on_path_enter(self, event):
        target = self.path_var.get().strip()
        if target.lower() in ("this pc", "thispc", "computer"):
            target = THIS_PC
        elif target.lower() in ("recycle bin", "recyclebin", "trash"):
            target = RECYCLE
        if self.navigate(target):
            self.tree.focus_set()

    def on_path_escape(self, event):
        self.path_var.set(self.cwd)
        self.tree.focus_set()

    def focus_filter(self):
        self.filter_entry.focus_set()
        self.filter_entry.selection_range(0, "end")

    def on_filter_change(self, *args):
        if not self._suppress_filter:
            self.apply_filter_and_sort()

    def on_filter_escape(self, event):
        self.filter_var.set("")
        self.tree.focus_set()

    def on_escape(self):
        if self.filter_var.get():
            self.filter_var.set("")
        else:
            self.tree.focus_set()

    def on_typeahead(self, event):
        if event.state & ALT_MASK or not event.char or not event.char.isprintable():
            return
        now = time.time()
        if now - self._ta_time > 1.0:
            self._ta_buf = ""
        self._ta_time = now
        self._ta_buf += event.char.lower()
        for idx, item in enumerate(self.shown):
            if item.name.lower().startswith(self._ta_buf):
                self.jump(idx)
                break

    # ------------------------------------------------------------ Tabs Control
    def new_tab(self):
        new_t = Tab(self.cwd or os.path.expanduser("~"))
        self.tabs.append(new_t)
        self.notebook.add(ttk.Frame(self.notebook, height=1), text="New Tab")
        self.cur = len(self.tabs) - 1
        self.notebook.select(self.cur)
        self.navigate(new_t.cwd, record=False)

    def close_tab(self):
        if len(self.tabs) <= 1:
            return
        del self.tabs[self.cur]
        self.notebook.forget(self.cur)
        self.cur = min(self.cur, len(self.tabs) - 1)
        self.notebook.select(self.cur)
        self.navigate(self.tabs[self.cur].cwd, record=False)

    def next_tab(self, step=1):
        if len(self.tabs) <= 1:
            return
        self.cur = (self.cur + step) % len(self.tabs)
        self.notebook.select(self.cur)

    def goto_tab(self, index):
        if 0 <= index < len(self.tabs):
            self.cur = index
            self.notebook.select(self.cur)

    def on_tab_changed(self, event):
        idx = self.notebook.index("current")
        if idx != self.cur and idx < len(self.tabs):
            if 0 <= self.cur < len(self.tabs):
                old_tab = self.tabs[self.cur]
                old_tab.cwd = self.cwd
                old_tab.back = self.back
                old_tab.fwd = self.fwd
                old_tab.sort_col = self.sort_col
                old_tab.sort_reverse = self.sort_reverse
                old_tab.filter = self.filter_var.get()
            self.cur = idx
            t = self.tabs[self.cur]
            self.cwd = t.cwd
            self.back = t.back
            self.fwd = t.fwd
            self.sort_col = t.sort_col
            self.sort_reverse = t.sort_reverse
            self._suppress_filter = True
            self.filter_var.set(t.filter)
            self._suppress_filter = False
            self.navigate(t.cwd, record=False)

    def on_tab_middle_click(self, event):
        element = self.notebook.identify(event.x, event.y)
        if "label" in element:
            index = self.notebook.index(f"@{event.x},{event.y}")
            if index < len(self.tabs):
                self.cur = index
                self.close_tab()

    # ------------------------------------------------------- Sidebar Actions
    def toggle_sidebar_focus(self):
        if self.root.focus_get() == self.sidebar:
            self.tree.focus_set()
        else:
            self.sidebar.focus_set()

    def focus_tree(self):
        self.tree.focus_set()

    def side_activate(self):
        curr = self.sidebar.focus()
        if curr in self.side_paths:
            self.navigate(self.side_paths[curr])
            self.tree.focus_set()

    def on_side_click(self, event):
        item = self.sidebar.identify_row(event.y)
        if item and item in self.side_paths:
            self.navigate(self.side_paths[item])

    def pin_path(self, path):
        if path in VIRTUAL or not os.path.isdir(path):
            return
        pins = self.cfg.setdefault("pins", [])
        if path not in pins:
            pins.append(path)
            save_config(self.cfg)
            self.build_sidebar()
            self.flash(f"Pinned: {path}")

    def pin_current(self):
        self.pin_path(self.cwd)

    def unpin_path(self, path):
        pins = self.cfg.get("pins", [])
        if path in pins:
            pins.remove(path)
            save_config(self.cfg)
            self.build_sidebar()
            self.flash("Unpinned item")

    def unpin_current(self):
        self.unpin_path(self.cwd)

    def unpin_side(self):
        curr = self.sidebar.focus()
        if curr in self.side_paths:
            p = self.side_paths[curr]
            self.unpin_path(p)

    def on_side_context(self, event):
        item = self.sidebar.identify_row(event.y)
        if not item or item not in self.side_paths:
            return
        self.sidebar.focus(item)
        self.sidebar.selection_set(item)
        p = self.side_paths[item]
        m = self.new_menu(self.sidebar)
        menu_add(m, "Open", lambda: self.navigate(p))
        if p in self.cfg.get("pins", []):
            menu_add(m, "Unpin", self.unpin_side)
        m.tk_popup(event.x_root, event.y_root)

    # --------------------------------------------------- Clipboard & Operations
    def selected_items(self):
        sel = self.tree.selection()
        return [self.shown[int(i)] for i in sel if i.isdigit()]

    def select_all(self):
        all_iids = [str(i) for i in range(len(self.shown))]
        self.tree.selection_set(all_iids)

    def copy_sel(self, mode="copy"):
        items = self.selected_items()
        if not items or self.cwd in VIRTUAL:
            return
        self.clip = [i.path for i in items]
        self.clip_mode = mode
        self.flash(f"{mode.capitalize()}ed {len(items)} item(s)")

    def copy_path(self):
        items = self.selected_items()
        if not items:
            return
        paths = "\n".join(i.path for i in items)
        self.root.clipboard_clear()
        self.root.clipboard_append(paths)
        self.flash(f"Copied {len(items)} path(s) to clipboard")

    def paste(self):
        if not self.clip or self.cwd in VIRTUAL:
            return
        count = 0
        for src in self.clip:
            if not os.path.exists(src):
                continue
            name = os.path.basename(src)
            dest = unique_path(self.cwd, name, os.path.isdir(src))
            try:
                if self.clip_mode == "cut":
                    shutil.move(src, dest)
                else:
                    if os.path.isdir(src):
                        shutil.copytree(src, dest)
                    else:
                        shutil.copy2(src, dest)
                count += 1
            except Exception as err:
                self.flash(f"Paste error: {err}")
        if self.clip_mode == "cut":
            self.clip.clear()
        self.reload()
        self.flash(f"Pasted {count} item(s)")

    def delete_sel(self, permanent=False):
        items = self.selected_items()
        if not items:
            return
        if self.cwd == RECYCLE:
            permanent = True

        mode_text = "permanently delete" if permanent else "move to Recycle Bin"
        if len(items) > 1:
            msg = f"Are you sure you want to {mode_text} {len(items)} items?"
        else:
            msg = f"Are you sure you want to {mode_text} '{items[0].name}'?"

        if not messagebox.askyesno("Confirm Delete", msg, parent=self.root):
            return

        for item in items:
            try:
                if permanent:
                    remove_path(item.path)
                    if item.info:
                        remove_path(item.info)
                else:
                    move_to_trash(item.path)
            except Exception as err:
                self.flash(f"Delete error: {err}")

        self.reload()

    def restore_selected(self):
        if self.cwd != RECYCLE:
            return
        items = self.selected_items()
        restored = 0
        for item in items:
            if not item.orig:
                continue
            dest_dir = os.path.dirname(item.orig)
            if not dest_dir or not os.path.isdir(dest_dir):
                dest_dir = os.path.expanduser("~")
            dest = unique_path(dest_dir, item.name, item.is_dir)
            try:
                shutil.move(item.path, dest)
                if item.info:
                    remove_path(item.info)
                restored += 1
            except Exception as err:
                self.flash(f"Restore error: {err}")
        self.reload()
        self.flash(f"Restored {restored} item(s)")

    def empty_bin(self):
        if not messagebox.askyesno("Empty Recycle Bin",
                                      "Permanently delete all items in the Recycle Bin?",
                                      parent=self.root):
            return
        trash = scan_trash()
        removed = failed = 0
        for item in trash:
            try:
                remove_path(item.path)
                if item.info and os.path.lexists(item.info):
                    remove_path(item.info)
                removed += 1
            except Exception:
                failed += 1
        self.reload()
        self.flash(f"Recycle Bin: removed {removed}, failed {failed}")

    def rename(self):
        items = self.selected_items()
        if len(items) != 1 or self.cwd in VIRTUAL:
            return
        item = items[0]

        dialog = tk.Toplevel(self.root)
        dialog.title("Rename")
        dialog.geometry("360x120")
        dialog.transient(self.root)
        dialog.grab_set()

        ttk.Label(dialog, text=f"New name for '{item.name}':").pack(anchor="w", padx=10, pady=(10, 4))
        var = tk.StringVar(value=item.name)
        entry = ttk.Entry(dialog, textvariable=var)
        entry.pack(fill="x", padx=10, pady=4)
        entry.focus_set()
        entry.selection_range(0, "end")

        def submit():
            new_name = var.get().strip()
            if new_name and new_name != item.name:
                new_path = os.path.join(self.cwd, new_name)
                try:
                    os.rename(item.path, new_path)
                    self.reload()
                except Exception as err:
                    self.flash(f"Rename error: {err}")
            dialog.destroy()

        entry.bind("<Return>", lambda e: submit())
        entry.bind("<Escape>", lambda e: dialog.destroy())
        ttk.Button(dialog, text="OK", command=submit).pack(side="right", padx=10, pady=10)

    def new_entry(self, is_dir=False):
        if self.cwd in VIRTUAL:
            return
        kind = "folder" if is_dir else "file"
        dialog = tk.Toplevel(self.root)
        dialog.title(f"New {kind}")
        dialog.geometry("360x120")
        dialog.transient(self.root)
        dialog.grab_set()

        ttk.Label(dialog, text=f"Name for new {kind}:").pack(anchor="w", padx=10, pady=(10, 4))
        default_name = "New Folder" if is_dir else "New File.txt"
        var = tk.StringVar(value=default_name)
        entry = ttk.Entry(dialog, textvariable=var)
        entry.pack(fill="x", padx=10, pady=4)
        entry.focus_set()
        entry.selection_range(0, "end")

        def submit():
            name = var.get().strip()
            if name:
                target = os.path.join(self.cwd, name)
                try:
                    if is_dir:
                        os.makedirs(target, exist_ok=False)
                    else:
                        with open(target, "x") as f:
                            pass
                    self.reload()
                except Exception as err:
                    self.flash(f"Create error: {err}")
            dialog.destroy()

        entry.bind("<Return>", lambda e: submit())
        entry.bind("<Escape>", lambda e: dialog.destroy())
        ttk.Button(dialog, text="OK", command=submit).pack(side="right", padx=10, pady=10)

    def properties_sel(self):
        items = self.selected_items()
        if not items:
            return
        item = items[0]
        p_win = tk.Toplevel(self.root)
        p_win.title("Properties")
        p_win.geometry("340x260")
        p_win.transient(self.root)

        txt = f"Name: {item.name}\n"
        txt += f"Path: {item.path}\n"
        txt += f"Type: {'Folder' if item.is_dir else 'File'}\n"
        if not item.is_dir:
            txt += f"Size: {human_size(item.size)}\n"
        if item.mtime:
            txt += f"Modified: {fmt_time(item.mtime)}\n"
        if item.orig:
            txt += f"Original: {item.orig}\n"

        lbl = ttk.Label(p_win, text=txt, justify="left", padding=15)
        lbl.pack(fill="both", expand=True)

    # ------------------------------------------------------------- Menus & Help
    def on_tree_context(self, event):
        row = self.tree.identify_row(event.y)
        if row:
            self.tree.focus(row)
            if row not in self.tree.selection():
                self.tree.selection_set(row)
        else:
            self.tree.selection_remove(self.tree.selection())
            self.tree.focus("")
        self.show_context_menu(event.x_root, event.y_root)

    def context_from_keyboard(self):
        curr = self.tree.focus()
        if curr:
            bbox = self.tree.bbox(curr)
            if bbox:
                x = self.tree.winfo_rootx() + bbox[0] + 20
                y = self.tree.winfo_rooty() + bbox[1] + 10
                self.show_context_menu(x, y)

    def show_context_menu(self, x, y):
        m = self.ctx
        m.delete(0, "end")
        items = self.selected_items()
        if self.cwd == RECYCLE:
            if items:
                menu_add(m, "Restore", self.restore_selected)
                menu_add(m, "Delete permanently",
                         lambda: self.delete_sel(permanent=True))
            menu_add(m, "Empty Recycle Bin", self.empty_bin)
        else:
            if items:
                item = items[0]
                menu_add(m, "Open", self.open_focused)
                if not item.is_dir and len(items) == 1:
                    menu_add(m, "Open with…", lambda p=item.path: self.open_with(p))
                m.add_separator()
                menu_add(m, "Cut", lambda: self.copy_sel("cut"), "Ctrl+X")
                menu_add(m, "Copy", lambda: self.copy_sel("copy"), "Ctrl+C")
                menu_add(m, "Copy path", self.copy_path, "Ctrl+Shift+C")
                if len(items) == 1 and item.is_dir and self.cwd not in VIRTUAL:
                    if item.path in self.cfg.get("pins", []):
                        menu_add(m, "Unpin from Quick access",
                                 lambda p=item.path: self.unpin_path(p))
                    else:
                        menu_add(m, "Pin to Quick access",
                                 lambda p=item.path: self.pin_path(p))
                menu_add(m, "Rename", self.rename, "F2")
                menu_add(m, "Delete", self.delete_sel, "Delete")
                menu_add(m, "Properties", self.properties_sel, "Alt+Enter")
                m.add_separator()
            if self.clip and self.cwd not in VIRTUAL:
                menu_add(m, "Paste", self.paste, "Ctrl+V")
            menu_add(m, "New folder", lambda: self.new_entry(True))
            menu_add(m, "New file", lambda: self.new_entry(False))
            m.add_separator()
            menu_add(m, "Refresh", self.reload, "F5")
        try:
            m.tk_popup(x, y)
        finally:
            m.grab_release()

    def open_with(self, path):
        """Open the OS app chooser on Windows or accept a custom app command."""
        if IS_WIN:
            try:
                os.startfile(path, "openas")
                return
            except Exception:
                pass
        command = simpledialog.askstring(
            "Open with",
            "Enter an application command (use {file} for the file path):",
            parent=self.root)
        if not command:
            return
        import shlex
        try:
            parts = shlex.split(command)
            has_placeholder = any("{file}" in part for part in parts)
            args = [part.replace("{file}", path) for part in parts]
            if not has_placeholder:
                args.append(path)
            subprocess.Popen(args)
        except Exception as err:
            self.flash(f"Open with error: {err}")

    def show_help(self):
        h_win = tk.Toplevel(self.root)
        h_win.title("Keyboard Shortcuts")
        h_win.geometry("540x500")

        txt = tk.Text(h_win, wrap="word", padx=10, pady=10,
                      bg=self.palette["field"], fg=self.palette["fg"])
        txt.pack(fill="both", expand=True)

        for category, shortcuts in SHORTCUTS:
            txt.insert("end", f"=== {category} ===\n", "head")
            for key, desc in shortcuts:
                txt.insert("end", f"{key:<26} {desc}\n")
            txt.insert("end", "\n")

        txt.tag_config("head", font=("TkDefaultFont", 10, "bold"))
        txt.configure(state="disabled")

    def flash(self, text, ms=3000):
        self.status_var.set(text)

    def update_status(self):
        total = len(self.shown)
        sel = len(self.tree.selection())
        status = f"{total} items"
        if sel > 0:
            status += f" ({sel} selected)"
        self.status_var.set(status)


if __name__ == "__main__":
    root = tk.Tk()
    start_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~")
    app = FileManager(root, start_dir)
    root.mainloop()
