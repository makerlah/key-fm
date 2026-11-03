#!/usr/bin/env python3
"""
keyfm - a keyboard-driven file manager with browser-style tabs (+ / x), a
modern dark/light look, a sidebar with pin/unpin-able Quick access and drive
usage, a working Recycle Bin, an "Open with" dialog listing every installed
app, fully editable keyboard shortcuts (Shortcuts menu / F1) and click-and-drag
selection boxes.
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
import shlex
import shutil
import stat
import struct
import subprocess
import sys
import threading
import time
import tkinter as tk
from datetime import datetime
from tkinter import ttk, messagebox, filedialog
from tkinter import font as tkfont
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

LIGHT = dict(bg="#f3f4f7", fg="#1c1e26", field="#ffffff", side="#eceef3",
             sel_bg="#dbe7ff", sel_fg="#101319", head="#f3f4f7",
             border="#dcdfe6", entry="#ffffff", tab="#e4e7ee",
             tab_sel="#ffffff", menu_bg="#ffffff", menu_active="#e6edff",
             dim="#7a8090", scroll="#c4c9d4", accent="#3b6ef5",
             accent_fg="#ffffff", hover="#e6e9f0", danger="#e5484d")
DARK = dict(bg="#1a1b20", fg="#e6e8ee", field="#1f2026", side="#16171b",
            sel_bg="#2c446e", sel_fg="#ffffff", head="#1a1b20",
            border="#2b2d35", entry="#25272f", tab="#23252c",
            tab_sel="#1f2026", menu_bg="#25272f", menu_active="#33363f",
            dim="#8b90a0", scroll="#3b3e49", accent="#6b9bff",
            accent_fg="#0d1220", hover="#2a2c34", danger="#ff6b70")

QUICK_ICONS = {"Home": "🏠", "Desktop": "🖥", "Downloads": "📥",
               "Documents": "🗂", "Pictures": "🖼", "Music": "🎵",
               "Videos": "🎬", "Recycle Bin": "🗑"}

# (id, label, category, default shortcuts, scope)
# scope "all": works everywhere (plain keys like Backspace only in the file
# list so they never break typing); scope "list": only in the file list.
ACTION_DEFS = [
    ("new_tab", "New tab", "Tabs", ["Ctrl+T"], "all"),
    ("close_tab", "Close tab", "Tabs", ["Ctrl+W"], "all"),
    ("next_tab", "Next tab", "Tabs", ["Ctrl+Tab", "Ctrl+PgDn"], "all"),
    ("prev_tab", "Previous tab", "Tabs", ["Ctrl+Shift+Tab", "Ctrl+PgUp"], "all"),

    ("open", "Open selected item", "Navigation", ["Enter"], "list"),
    ("open_folder", "Open folder only", "Navigation", ["Right"], "list"),
    ("go_parent", "Up one folder", "Navigation", ["Backspace", "Left", "Alt+Up"], "all"),
    ("back", "Back", "Navigation", ["Alt+Left"], "all"),
    ("forward", "Forward", "Navigation", ["Alt+Right"], "all"),
    ("home", "Go to home folder", "Navigation", ["Alt+Home"], "all"),
    ("this_pc", "Go to This PC (drives)", "Navigation", ["Ctrl+Shift+M"], "all"),
    ("recycle", "Go to Recycle Bin", "Navigation", ["Ctrl+Shift+B"], "all"),
    ("edit_path", "Edit the path", "Navigation", ["Ctrl+L"], "all"),
    ("filter", "Filter this folder", "Navigation", ["Ctrl+F"], "all"),
    ("sidebar", "Jump to sidebar and back", "Navigation", ["Ctrl+B"], "all"),
    ("first", "First item", "Navigation", ["Home"], "list"),
    ("last", "Last item", "Navigation", ["End"], "list"),
    ("page_up", "Move up 15 rows", "Navigation", ["PgUp"], "list"),
    ("page_down", "Move down 15 rows", "Navigation", ["PgDn"], "list"),

    ("select_all", "Select all", "Files", ["Ctrl+A"], "list"),
    ("copy", "Copy", "Files", ["Ctrl+C"], "list"),
    ("cut", "Cut", "Files", ["Ctrl+X"], "list"),
    ("paste", "Paste", "Files", ["Ctrl+V"], "list"),
    ("copy_path", "Copy path", "Files", ["Ctrl+Shift+C"], "list"),
    ("rename", "Rename", "Files", ["F2"], "list"),
    ("delete", "Delete (to Recycle Bin)", "Files", ["Delete"], "list"),
    ("delete_perm", "Delete permanently", "Files", ["Shift+Delete"], "list"),
    ("new_file", "New file", "Files", ["Ctrl+N"], "list"),
    ("new_folder", "New folder", "Files", ["Ctrl+Shift+N"], "list"),
    ("properties", "Properties", "Files", ["Alt+Enter"], "list"),
    ("open_with", "Open with…", "Files", ["Ctrl+Shift+O"], "all"),
    ("context_menu", "Right-click menu", "Files", ["Shift+F10", "Menu"], "list"),
    ("restore", "Restore from Recycle Bin", "Files", ["Ctrl+Z"], "list"),
    ("empty_bin", "Empty Recycle Bin", "Files", ["Ctrl+Shift+Delete"], "all"),

    ("pin", "Pin / unpin this folder (Quick access)", "Quick access", ["Ctrl+P"], "all"),
    ("unpin", "Unpin this folder", "Quick access", ["Ctrl+Shift+P"], "all"),

    ("hidden", "Show / hide hidden files", "View", ["Ctrl+H"], "all"),
    ("dark", "Dark / light mode", "View", ["Ctrl+Shift+D"], "all"),
    ("sort_name", "Sort by name", "View", ["Ctrl+1"], "all"),
    ("sort_size", "Sort by size", "View", ["Ctrl+2"], "all"),
    ("sort_date", "Sort by date", "View", ["Ctrl+3"], "all"),
    ("refresh", "Refresh", "View", ["F5", "Ctrl+R"], "all"),

    ("shortcuts", "Keyboard shortcuts…", "Program", ["F1"], "all"),
    ("menu_bar", "Open the File menu", "Program", ["F10"], "all"),
    ("quit", "Quit", "Program", ["Ctrl+Q"], "all"),
]
ACTION_INFO = {a[0]: a for a in ACTION_DEFS}
ENTRY_PASS = {"hidden"}   # leave these to text boxes (Ctrl+H = Backspace there)

# built-in keys that can't be changed (shown read-only in the editor)
FIXED_KEYS = [
    ("Up / Down", "Move the selection"),
    ("Shift+Up / Shift+Down", "Extend the selection"),
    ("Ctrl+Up / Ctrl+Down", "Move focus without selecting"),
    ("Space", "Toggle the focused item"),
    ("Esc", "Clear the filter / back to the list"),
    ("<type letters>", "Jump to the item that starts with them"),
    ("Alt+1 … Alt+9", "Go to tab number 1-9"),
    ("Alt+F / E / V / G / S", "Open the menus"),
    ("Enter (sidebar)", "Open the highlighted entry"),
    ("Esc (sidebar)", "Back to the file list"),
    ("Delete (sidebar)", "Remove the entry from Quick access"),
    ("Click + drag", "Draw a selection box around files"),
]

RESERVED = {"Up", "Down", "Esc", "Space", "Tab", "Shift+Up", "Shift+Down",
            "Ctrl+Up", "Ctrl+Down", "Alt+Tab", "Alt+F4"}
RESERVED |= {f"Alt+{n}" for n in range(1, 10)}
RESERVED |= {f"Alt+{c}" for c in "FEVGS"}

MOD_ORDER = ("Ctrl", "Alt", "Shift")
KEY_ALIASES = {"PgUp": "Prior", "PgDn": "Next", "Enter": "Return",
               "Backspace": "BackSpace", "Esc": "Escape", "Space": "space"}
KEY_DISPLAY = {"Prior": "PgUp", "Next": "PgDn", "Return": "Enter",
               "KP_Enter": "Enter", "BackSpace": "Backspace",
               "Escape": "Esc", "space": "Space", "ISO_Left_Tab": "Tab"}
MOD_SYMS = {"Shift_L", "Shift_R", "Control_L", "Control_R", "Alt_L", "Alt_R",
            "Meta_L", "Meta_R", "Super_L", "Super_R", "Caps_Lock", "Num_Lock",
            "ISO_Level3_Shift", "Mode_switch"}


def split_key(spec):
    parts = [p for p in str(spec).split("+") if p]
    mods = {p.capitalize() for p in parts[:-1] if p.capitalize() in MOD_ORDER}
    return mods, (parts[-1] if parts else "")


def norm_spec(spec):
    mods, key = split_key(str(spec).strip())
    if not key:
        return ""
    key = KEY_DISPLAY.get(KEY_ALIASES.get(key, key), key)
    if len(key) == 1 and key.isalpha():
        key = key.upper()
    return "+".join([m for m in MOD_ORDER if m in mods] + [key])


def is_safe_key(spec):
    """Safe to bind on the whole window (won't fire while typing text)."""
    mods, key = split_key(spec)
    return ("Ctrl" in mods or "Alt" in mods
            or re.fullmatch(r"F\d{1,2}", key) is not None)


def key_sequences(spec):
    """Tk event sequences for a shortcut such as 'Ctrl+Shift+T'."""
    mods, key = split_key(spec)
    keysym = KEY_ALIASES.get(key, key)
    prefix = ""
    if "Ctrl" in mods:
        prefix += "Control-"
    if "Alt" in mods:
        prefix += "Alt-"
    if "Shift" in mods:
        prefix += "Shift-"
    syms = [keysym]
    if len(keysym) == 1 and keysym.isalpha():
        syms = [keysym.lower(), keysym.upper()]
    elif keysym == "Return":
        syms.append("KP_Enter")
    elif keysym == "Tab" and "Shift" in mods:
        syms.append("ISO_Left_Tab")
    elif keysym == "Menu":
        syms.append("App")
    return [f"<{prefix}Key-{x}>" for x in syms]


def event_to_spec(event):
    """Turn a key press into 'Ctrl+Shift+T' style text (None for bare modifiers)."""
    sym = event.keysym
    if sym in MOD_SYMS:
        return None
    mods = []
    if event.state & 0x4:
        mods.append("Ctrl")
    if event.state & ALT_MASK:
        mods.append("Alt")
    shift = bool(event.state & 0x1)
    if sym == "ISO_Left_Tab":
        sym, shift = "Tab", True
    if shift:
        mods.append("Shift")
    key = KEY_DISPLAY.get(sym, sym)
    if len(key) == 1 and key.isalpha():
        key = key.upper()
    return "+".join(mods + [key])


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
        self.sel = []        # selected paths, restored when you come back
        self.focus = None    # focused path


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
def _valid_assoc(v):
    return (isinstance(v, dict) and isinstance(v.get("name"), str)
            and isinstance(v.get("argv"), list)
            and all(isinstance(a, str) for a in v["argv"]))


def load_config():
    cfg = {"dark": True, "show_hidden": False, "pins": [], "unpinned": [],
           "assoc": {}, "keys": {}}
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
        if isinstance(data.get("assoc"), dict):
            cfg["assoc"] = {k: v for k, v in data["assoc"].items()
                            if isinstance(k, str) and _valid_assoc(v)}
        if isinstance(data.get("keys"), dict):
            for aid, lst in data["keys"].items():
                if aid in ACTION_INFO and isinstance(lst, list):
                    good = []
                    for k in lst:
                        k = norm_spec(k) if isinstance(k, str) else ""
                        if k and k not in good:
                            good.append(k)
                    cfg["keys"][aid] = good
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
            f.write("[Trash Info]\n")
            f.write("Path=" + quote(os.path.abspath(path), safe="/") + "\n")
            f.write("DeletionDate=" + datetime.now().strftime("%Y-%m-%dT%H:%M:%S") + "\n")
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


# ------------------------------------------------------- installed apps
# An app is {"name": str, "argv": [program, ..., "{file}"]}; "{file}" is
# replaced by the file path when launching.
def parse_desktop(path):
    """Parse one Linux .desktop file into an app dict (or None)."""
    data = {}
    in_entry = False
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                line = line.strip()
                if line.startswith("["):
                    in_entry = line == "[Desktop Entry]"
                    continue
                if not in_entry or line.startswith("#") or "=" not in line:
                    continue
                k, v = line.split("=", 1)
                if k in ("Name", "Exec", "Type", "NoDisplay", "Hidden",
                         "Terminal") and k not in data:
                    data[k] = v.strip()
    except OSError:
        return None
    if data.get("Type") != "Application" or not data.get("Exec") or not data.get("Name"):
        return None
    if any(data.get(k, "").lower() == "true" for k in ("NoDisplay", "Hidden", "Terminal")):
        return None
    try:
        parts = shlex.split(data["Exec"])
    except ValueError:
        return None
    args, has_file = [], False
    for p in parts:
        q = p.replace("%%", "\0")
        if re.search(r"%[fFuU]", q):
            has_file = True
            q = re.sub(r"%[fFuU]", "{file}", q)
        q = re.sub(r"%[a-zA-Z]", "", q)
        q = q.replace("\0", "%")
        if q == "" and p != "":
            continue  # token was only an unsupported field code (e.g. %i)
        args.append(q)
    if not args:
        return None
    if not has_file:
        args.append("{file}")
    return {"name": data["Name"], "argv": args}


def linux_apps():
    home = os.path.expanduser("~")
    data_home = os.environ.get("XDG_DATA_HOME") or os.path.join(home, ".local", "share")
    dirs = [os.path.join(data_home, "applications")]
    for d in (os.environ.get("XDG_DATA_DIRS") or "/usr/local/share:/usr/share").split(":"):
        if d:
            dirs.append(os.path.join(d, "applications"))
    dirs += ["/var/lib/flatpak/exports/share/applications",
             os.path.join(data_home, "flatpak", "exports", "share", "applications"),
             "/var/lib/snapd/desktop/applications"]
    seen, apps = set(), []
    for d in dirs:
        if not os.path.isdir(d):
            continue
        for root_, _dirs, files in os.walk(d):
            for fn in files:
                if not fn.endswith(".desktop") or fn in seen:
                    continue
                seen.add(fn)
                app = parse_desktop(os.path.join(root_, fn))
                if app:
                    apps.append(app)
    return apps


def mac_apps():
    home = os.path.expanduser("~")
    apps = []
    for d in ("/Applications", "/Applications/Utilities", "/System/Applications",
              "/System/Applications/Utilities", os.path.join(home, "Applications")):
        try:
            names = os.listdir(d)
        except OSError:
            continue
        for n in names:
            if n.endswith(".app"):
                apps.append({"name": n[:-4],
                             "argv": ["open", "-a", os.path.join(d, n), "{file}"]})
    return apps


def win_apps():
    """Start Menu shortcuts (resolved with PowerShell) + registered App Paths."""
    apps = {}

    def add(name, exe):
        low = exe.lower()
        base = os.path.basename(low)
        if not low.endswith(".exe") or low in apps or not os.path.isfile(exe):
            return
        if "uninst" in base or "uninst" in name.lower() or base.startswith("unins"):
            return
        apps[low] = {"name": name, "argv": [exe, "{file}"]}

    dirs = []
    for env in ("ProgramData", "APPDATA"):
        base = os.environ.get(env)
        if base:
            d = os.path.join(base, "Microsoft", "Windows", "Start Menu", "Programs")
            if os.path.isdir(d):
                dirs.append(d)
    if dirs:
        quoted = ",".join("'" + d.replace("'", "''") + "'" for d in dirs)
        script = ("[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;"
                  "$sh=New-Object -ComObject WScript.Shell;"
                  "Get-ChildItem -LiteralPath " + quoted +
                  " -Recurse -Filter *.lnk -ErrorAction SilentlyContinue | "
                  "ForEach-Object { $t=$sh.CreateShortcut($_.FullName).TargetPath; "
                  "if($t -like '*.exe'){ $_.BaseName + '|' + $t } }")
        try:
            out = subprocess.run(
                ["powershell", "-NoProfile", "-NonInteractive", "-Command", script],
                capture_output=True, timeout=90,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)).stdout
            rows = []
            for line in out.decode("utf-8", "replace").splitlines():
                name, _, target = line.partition("|")
                if name.strip() and target.strip():
                    rows.append((name.strip(), target.strip()))
            for name, target in sorted(rows, key=lambda r: r[0].lower()):
                add(name, target)
        except Exception:
            pass

    try:
        import winreg
        for hive in (winreg.HKEY_LOCAL_MACHINE, winreg.HKEY_CURRENT_USER):
            try:
                key = winreg.OpenKey(
                    hive, r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths")
            except OSError:
                continue
            try:
                i = 0
                while True:
                    try:
                        sub = winreg.EnumKey(key, i)
                    except OSError:
                        break
                    i += 1
                    try:
                        with winreg.OpenKey(key, sub) as sk:
                            val, _t = winreg.QueryValueEx(sk, "")
                    except OSError:
                        continue
                    exe = os.path.expandvars(str(val)).strip().strip('"')
                    name = os.path.splitext(sub)[0]
                    add(name[:1].upper() + name[1:], exe)
            finally:
                winreg.CloseKey(key)
    except Exception:
        pass

    sysroot = os.environ.get("SystemRoot", r"C:\Windows")
    for name, exe in (("Notepad", "notepad.exe"), ("Paint", "mspaint.exe"),
                      ("WordPad", "write.exe")):
        add(name, os.path.join(sysroot, "System32", exe))
    if not any(a["name"] == "Notepad" for a in apps.values()):
        add("Notepad", os.path.join(sysroot, "notepad.exe"))
    return list(apps.values())


def list_apps():
    try:
        if IS_WIN:
            apps = win_apps()
        elif sys.platform == "darwin":
            apps = mac_apps()
        else:
            apps = linux_apps()
    except Exception:
        apps = []
    apps.sort(key=lambda a: a["name"].lower())
    return apps


def app_from_file(fn):
    """Build an app dict from a program the user browsed to."""
    base = os.path.splitext(os.path.basename(fn.rstrip("/\\")))[0] or fn
    if sys.platform == "darwin" and fn.rstrip("/").endswith(".app"):
        return {"name": base, "argv": ["open", "-a", fn, "{file}"]}
    return {"name": base, "argv": [fn, "{file}"]}


def launch_with(argv, path):
    args = [a.replace("{file}", path) for a in argv]
    if IS_WIN:
        subprocess.Popen(args)
    else:
        subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL, start_new_session=True)


def default_open(path):
    if IS_WIN:
        os.startfile(path)
    elif sys.platform == "darwin":
        subprocess.Popen(["open", path])
    else:
        subprocess.Popen(["xdg-open", path], stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         start_new_session=True)


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
        self.drive_rows = []   # (sidebar iid, drive name, path)
        self.usage = {}        # path -> (used bytes, total bytes)
        self._usage_busy = False
        self._usage_last = 0.0
        self._apps = None      # installed apps; None while still loading
        self.menus = []
        self.menubuttons = []
        self.menu_keys = []
        self.flat_btns = []
        self.side_kind = {}    # sidebar iid -> ("default", label) / ("pin", path)
        self._bound = []       # (widget, sequence) pairs bound for shortcuts
        self.funcs = {}
        self._lasso = None     # state of a click-and-drag selection box
        self.quick = known_folders()
        self.tabs = [Tab()]
        self.cur = 0

        self.build_funcs()
        self.build_ui()
        self.build_menus()
        self.apply_theme()
        self.bind_keys()
        self.apply_bindings()
        self.build_sidebar()
        threading.Thread(target=self._load_apps, daemon=True).start()
        home = os.path.expanduser("~")
        if not self.navigate(start, record=False):
            self.navigate(home, record=False)
        self.tree.focus_set()

    def _load_apps(self):
        try:
            apps = list_apps()
        except Exception:
            apps = []
        self._apps = apps

    # ------------------------------------------------------------------ UI
    def setup_fonts(self):
        fams = set(tkfont.families(self.root))
        family = None
        for cand in ("Segoe UI Variable Text", "Segoe UI", "SF Pro Text",
                     "Helvetica Neue", "Inter", "Ubuntu", "Noto Sans",
                     "Cantarell", "DejaVu Sans"):
            if cand in fams:
                family = cand
                break
        size = 13 if sys.platform == "darwin" else 10
        for name in ("TkDefaultFont", "TkTextFont", "TkMenuFont", "TkHeadingFont"):
            try:
                f = tkfont.nametofont(name)
                f.configure(size=size)
                if family:
                    f.configure(family=family)
            except tk.TclError:
                pass
        base = tkfont.nametofont("TkDefaultFont")
        fam = base.cget("family")
        self.base_font = base
        self.big_font = tkfont.Font(family=fam, size=size + 3)
        self.icon_font = tkfont.Font(family=fam, size=size + 4)
        self.small_bold = tkfont.Font(family=fam, size=max(8, size - 1),
                                      weight="bold")

    def flat_button(self, parent, text, cmd):
        """A small flat toolbar button (a Label that reacts to hover/click)."""
        b = tk.Label(parent, text=text, padx=10, pady=3, cursor="hand2",
                     font=self.icon_font)
        b.bind("<Button-1>", lambda e: self.root.after_idle(cmd))
        b.bind("<Enter>", lambda e: b.configure(bg=self.palette["hover"]))
        b.bind("<Leave>", lambda e: b.configure(bg=self.palette["bg"]))
        self.flat_btns.append(b)
        return b

    def add_sep(self, parent):
        sep = tk.Frame(parent, height=1)
        sep.pack(fill="x")
        self.seps.append(sep)

    def build_ui(self):
        r = self.root
        r.title(APP)
        r.geometry("1180x700")
        r.minsize(760, 420)
        self.seps = []
        self.setup_fonts()

        self.menubar = tk.Frame(r)
        self.menubar.pack(fill="x", padx=4, pady=(2, 0))

        # browser-style tab strip: [title x] [title x] [+]
        self.tabbar = tk.Frame(r)
        self.tabbar.pack(fill="x", padx=8, pady=(4, 0))
        self.add_sep(r)

        # toolbar: nav buttons, path, pin star, filter
        top = ttk.Frame(r, padding=(8, 8, 8, 8))
        top.pack(fill="x")
        for text, cmd in (("←", self.go_back), ("→", self.go_forward),
                          ("↑", self.go_parent), ("↻", self.reload)):
            self.flat_button(top, text, cmd).pack(side="left", padx=(0, 2))
        self.filter_var = tk.StringVar()
        self.filter_entry = ttk.Entry(top, textvariable=self.filter_var, width=22)
        self.filter_entry.pack(side="right")
        ttk.Label(top, text="🔍").pack(side="right", padx=(8, 4))
        self.btn_pin = self.flat_button(top, "★", self.toggle_pin_current)
        self.btn_pin.pack(side="right", padx=(6, 0))
        self.path_var = tk.StringVar()
        self.path_entry = ttk.Entry(top, textvariable=self.path_var)
        self.path_entry.pack(side="left", fill="x", expand=True, padx=(8, 0))

        mid = ttk.Frame(r)
        mid.pack(fill="both", expand=True, padx=8, pady=(0, 6))
        self.sidebar = ttk.Treeview(mid, show="tree", selectmode="browse",
                                    style="Side.Treeview")
        self.sidebar.column("#0", width=330, stretch=False)
        self.sidebar.pack(side="left", fill="y", padx=(0, 8))

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

        self.add_sep(r)
        self.status_var = tk.StringVar()
        ttk.Label(r, textvariable=self.status_var, anchor="w",
                  style="Status.TLabel").pack(fill="x")

        self.ctx = tk.Menu(r, tearoff=0)   # reusable right-click menu
        self.update_headings()

    def build_menus(self):
        self.hidden_var = tk.BooleanVar(master=self.root, value=self.show_hidden)
        self.dark_var = tk.BooleanVar(master=self.root, value=self.dark)
        self.menu_by_name = {}
        for label, key in (("File", "f"), ("Edit", "e"), ("View", "v"),
                           ("Go", "g"), ("Shortcuts", "s")):
            mb = tk.Menubutton(self.menubar, text=label, underline=0,
                               padx=10, pady=4, relief="flat", bd=0,
                               highlightthickness=0, takefocus=0)
            m = tk.Menu(mb, tearoff=0)
            mb.configure(menu=m)
            mb.pack(side="left")
            self.menubuttons.append(mb)
            self.menus.append(m)
            self.menu_keys.append((key, mb))
            self.menu_by_name[label] = m
        self.fill_menus()

    def accel(self, aid):
        keys = self.keys_for(aid) if aid in ACTION_INFO else []
        return keys[0] if keys else ""

    def ma(self, menu, label, func, aid=None):
        menu.add_command(label=label, command=func,
                         accelerator=self.accel(aid) if aid else "")

    def mi(self, menu, label, aid):
        self.ma(menu, label, self.funcs[aid], aid)

    def fill_menus(self):
        """(Re)build every menu; shows the CURRENT shortcut next to each item."""
        M, mi = self.menu_by_name, self.mi
        for m in M.values():
            m.delete(0, "end")

        f = M["File"]
        mi(f, "New tab", "new_tab")
        mi(f, "Close tab", "close_tab")
        mi(f, "Next tab", "next_tab")
        mi(f, "Previous tab", "prev_tab")
        f.add_separator()
        mi(f, "New file", "new_file")
        mi(f, "New folder", "new_folder")
        mi(f, "Open with…", "open_with")
        mi(f, "Properties", "properties")
        f.add_separator()
        mi(f, "Quit", "quit")

        e = M["Edit"]
        mi(e, "Cut", "cut")
        mi(e, "Copy", "copy")
        mi(e, "Paste", "paste")
        mi(e, "Copy path", "copy_path")
        e.add_separator()
        mi(e, "Select all", "select_all")
        mi(e, "Rename", "rename")
        e.add_separator()
        mi(e, "Delete (to Recycle Bin)", "delete")
        mi(e, "Delete permanently", "delete_perm")
        mi(e, "Restore from Recycle Bin", "restore")
        mi(e, "Empty Recycle Bin", "empty_bin")

        v = M["View"]
        v.add_checkbutton(label="Show hidden files", variable=self.hidden_var,
                          command=self.toggle_hidden,
                          accelerator=self.accel("hidden"))
        v.add_checkbutton(label="Dark mode", variable=self.dark_var,
                          command=self.toggle_dark,
                          accelerator=self.accel("dark"))
        v.add_separator()
        mi(v, "Sort by name", "sort_name")
        mi(v, "Sort by size", "sort_size")
        mi(v, "Sort by date", "sort_date")
        v.add_separator()
        mi(v, "Refresh", "refresh")

        g = M["Go"]
        mi(g, "Back", "back")
        mi(g, "Forward", "forward")
        mi(g, "Up", "go_parent")
        mi(g, "Home folder", "home")
        mi(g, "This PC", "this_pc")
        mi(g, "Recycle Bin", "recycle")
        g.add_separator()
        mi(g, "Edit path", "edit_path")
        mi(g, "Filter", "filter")
        mi(g, "Sidebar", "sidebar")
        g.add_separator()
        mi(g, "Pin / unpin this folder (Quick access)", "pin")
        mi(g, "Unpin this folder", "unpin")

        s = M["Shortcuts"]
        mi(s, "Edit keyboard shortcuts…", "shortcuts")
        s.add_separator()
        self.ma(s, "Reset all shortcuts to defaults…", self.reset_all_keys)

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
        st.configure("Dim.TLabel", background=P["bg"], foreground=P["dim"])
        st.configure("Status.TLabel", background=P["side"], foreground=P["dim"],
                     padding=(14, 6))
        st.configure("TCheckbutton", background=P["bg"], foreground=P["fg"],
                     indicatorbackground=P["entry"], indicatorforeground=P["fg"],
                     upperbordercolor=P["border"], lowerbordercolor=P["border"])
        st.map("TCheckbutton", background=[("active", P["bg"])],
               foreground=[("active", P["fg"]), ("disabled", P["dim"])])
        st.configure("TEntry", fieldbackground=P["entry"], foreground=P["fg"],
                     insertcolor=P["fg"], bordercolor=P["border"],
                     lightcolor=P["entry"], darkcolor=P["entry"],
                     padding=(8, 5))
        st.map("TEntry", bordercolor=[("focus", P["accent"])],
               lightcolor=[("focus", P["accent"])],
               darkcolor=[("focus", P["accent"])])
        st.configure("TButton", background=P["tab"], foreground=P["fg"],
                     bordercolor=P["tab"], lightcolor=P["tab"],
                     darkcolor=P["tab"], padding=(14, 6), relief="flat",
                     focusthickness=0)
        st.map("TButton",
               background=[("disabled", P["bg"]), ("pressed", P["sel_bg"]),
                           ("active", P["menu_active"])],
               foreground=[("disabled", P["dim"])],
               bordercolor=[("active", P["menu_active"])])
        st.configure("Accent.TButton", background=P["accent"],
                     foreground=P["accent_fg"], bordercolor=P["accent"],
                     lightcolor=P["accent"], darkcolor=P["accent"],
                     padding=(16, 6), focusthickness=0)
        st.map("Accent.TButton",
               background=[("disabled", P["tab"]), ("pressed", P["accent"]),
                           ("active", P["accent"])],
               foreground=[("disabled", P["dim"]), ("active", P["accent_fg"])],
               bordercolor=[("disabled", P["tab"])])
        for name, bgc in (("Treeview", P["field"]), ("Side.Treeview", P["side"])):
            st.configure(name, background=bgc, fieldbackground=bgc,
                         foreground=P["fg"], rowheight=30, bordercolor=bgc,
                         lightcolor=bgc, darkcolor=bgc, borderwidth=0)
            st.map(name, background=[("selected", P["sel_bg"])],
                   foreground=[("selected", P["sel_fg"])])
        st.configure("Side.Treeview", indent=14)
        st.configure("Treeview.Heading", background=P["head"], foreground=P["dim"],
                     relief="flat", bordercolor=P["border"], lightcolor=P["head"],
                     darkcolor=P["head"], padding=(8, 6))
        st.map("Treeview.Heading", background=[("active", P["hover"])],
               foreground=[("active", P["fg"])])
        st.configure("TScrollbar", background=P["scroll"], troughcolor=P["bg"],
                     bordercolor=P["bg"], arrowcolor=P["dim"],
                     lightcolor=P["scroll"], darkcolor=P["scroll"], arrowsize=12)
        st.map("TScrollbar", background=[("active", P["dim"])])

        self.root.configure(background=P["bg"])
        self.menubar.configure(bg=P["bg"])
        for mb in self.menubuttons:
            mb.configure(bg=P["bg"], fg=P["fg"], activebackground=P["menu_active"],
                         activeforeground=P["fg"])
        for m in self.menus + [self.ctx]:
            self.style_menu(m)
        for sep in self.seps:
            sep.configure(bg=P["border"])
        for b in self.flat_btns:
            b.configure(bg=P["bg"], fg=P["fg"])
        self.sidebar.tag_configure("head", foreground=P["dim"],
                                   font=self.small_bold)
        self.refresh_tabs()
        self.update_pin_button()

    def toggle_dark(self):
        self.dark = not self.dark
        self.dark_var.set(self.dark)
        self.apply_theme()
        self.cfg["dark"] = self.dark
        save_config(self.cfg)
        self.flash("Dark mode on" if self.dark else "Light mode on")

    # ---------------------------------------------------------------- keys
    def build_funcs(self):
        s = self
        self.funcs = {
            "new_tab": s.new_tab, "close_tab": s.close_tab,
            "next_tab": lambda: s.next_tab(1), "prev_tab": lambda: s.next_tab(-1),
            "open": s.open_focused,
            "open_folder": lambda: s.open_focused(dirs_only=True),
            "go_parent": s.go_parent, "back": s.go_back, "forward": s.go_forward,
            "home": lambda: s.navigate(os.path.expanduser("~")),
            "this_pc": lambda: s.navigate(THIS_PC),
            "recycle": lambda: s.navigate(RECYCLE),
            "edit_path": s.focus_path, "filter": s.focus_filter,
            "sidebar": s.toggle_sidebar_focus,
            "first": lambda: s.jump(0),
            "last": lambda: s.jump(len(s.shown) - 1),
            "page_up": lambda: s.move(-15), "page_down": lambda: s.move(15),
            "select_all": s.select_all,
            "copy": lambda: s.copy_sel("copy"), "cut": lambda: s.copy_sel("cut"),
            "paste": s.paste, "copy_path": s.copy_path, "rename": s.rename,
            "delete": s.delete_sel,
            "delete_perm": lambda: s.delete_sel(permanent=True),
            "new_file": lambda: s.new_entry(False),
            "new_folder": lambda: s.new_entry(True),
            "properties": s.properties_sel, "open_with": s.open_with_sel,
            "context_menu": s.context_from_keyboard,
            "restore": s.restore_selected, "empty_bin": s.empty_bin,
            "pin": s.toggle_pin_current, "unpin": s.unpin_current,
            "hidden": s.toggle_hidden, "dark": s.toggle_dark,
            "sort_name": lambda: s.set_sort("name"),
            "sort_size": lambda: s.set_sort("size"),
            "sort_date": lambda: s.set_sort("modified"),
            "refresh": s.reload, "shortcuts": s.show_shortcuts,
            "menu_bar": lambda: s.post_menu(s.menubuttons[0]),
            "quit": s.root.destroy,
        }

    def keys_for(self, aid):
        ov = self.cfg.get("keys", {}).get(aid)
        return list(ov) if ov is not None else list(ACTION_INFO[aid][3])

    def set_keys(self, aid, keys):
        clean = []
        for k in keys:
            if k and k not in clean:
                clean.append(k)
        store = self.cfg.setdefault("keys", {})
        if clean == list(ACTION_INFO[aid][3]):
            store.pop(aid, None)
        else:
            store[aid] = clean
        save_config(self.cfg)
        self.apply_bindings()
        self.fill_menus()

    def make_handler(self, aid, fn):
        def handler(event=None):
            if aid in ENTRY_PASS and isinstance(getattr(event, "widget", None),
                                                tk.Entry):
                return None   # let text boxes keep Ctrl+H etc.
            fn()
            return "break"
        return handler

    def apply_bindings(self):
        """(Re)bind every customizable shortcut from the current settings."""
        for w, seq in self._bound:
            try:
                w.bind(seq, "")
            except tk.TclError:
                pass
        self._bound = []
        r, t, sb = self.root, self.tree, self.sidebar
        for aid, _label, _cat, _defaults, scope in ACTION_DEFS:
            handler = self.make_handler(aid, self.funcs[aid])
            for spec in self.keys_for(aid):
                wide = scope == "all" and is_safe_key(spec)
                targets = (r, t, sb) if wide else (t,)
                for seq in key_sequences(spec):
                    for w in targets:
                        try:
                            w.bind(seq, handler)
                            self._bound.append((w, seq))
                        except tk.TclError:
                            pass

    def reset_all_keys(self, parent=None):
        if not messagebox.askyesno("Reset shortcuts",
                                   "Put every keyboard shortcut back to its default?",
                                   parent=parent or self.root):
            return False
        self.cfg["keys"] = {}
        save_config(self.cfg)
        self.apply_bindings()
        self.fill_menus()
        self.flash("Shortcuts reset to defaults")
        return True

    def bind_keys(self):
        r, t, sb = self.root, self.tree, self.sidebar

        def fixed(seq, fn):
            h = brk(fn)
            for w in (r, t, sb):
                w.bind(seq, h)

        # ---- file list: keys that can't be changed
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

        # ---- click-and-drag selection box
        t.bind("<ButtonPress-1>", self.lasso_press)
        t.bind("<B1-Motion>", self.lasso_motion)
        t.bind("<ButtonRelease-1>", self.lasso_release)

        # ---- tab numbers and menu letters
        for n in range(1, 10):
            fixed(f"<Alt-Key-{n}>", lambda n=n: self.goto_tab(n - 1))
        for key, mb in self.menu_keys:
            fixed(f"<Alt-{key}>", lambda mb=mb: self.post_menu(mb))

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

    # ------------------------------------------------ selection box (lasso)
    def _row_geom(self):
        """(first visible row index, y of that row, row height)."""
        t = self.tree
        n = len(self.shown)
        if not n:
            return 0, None, 1
        first = max(0, min(n - 1, int(round(t.yview()[0] * n))))
        bb = t.bbox(str(first))
        if not bb:
            return first, None, 1
        return first, bb[1], max(1, bb[3])

    def _set_selection(self, ids):
        t = self.tree
        cur, new = set(t.selection()), set(ids)
        if cur == new:
            return
        gone = cur - new
        if gone:
            t.selection_remove(*gone)
        add = new - cur
        if add:
            t.selection_add(*add)

    def lasso_press(self, event):
        self._lasso = None
        t = self.tree
        if t.identify_region(event.x, event.y) in ("heading", "separator"):
            return None
        if not self.shown:
            return None
        first, y0, rh = self._row_geom()
        if y0 is None:
            return None
        extend = bool(event.state & 0x4) or bool(event.state & 0x1)
        row = t.identify_row(event.y)
        self._lasso = {"x": event.x, "y": event.y,
                       "frow": first + (event.y - y0) / rh,   # anchor, in rows
                       "active": False, "last": None, "job": None,
                       "lines": None, "mx": event.x, "my": event.y,
                       "extend": extend,
                       "base": set(t.selection()) if extend else set()}
        if not row:                       # pressed on empty space
            t.focus_set()
            if not extend:
                cur = t.selection()
                if cur:
                    t.selection_remove(*cur)
            return "break"
        return None                       # normal click on a row

    def lasso_motion(self, event):
        L = self._lasso
        if not L:
            return None
        t = self.tree
        x = event.x_root - t.winfo_rootx()
        y = event.y_root - t.winfo_rooty()
        L["mx"], L["my"] = x, y
        if not L["active"]:
            if abs(x - L["x"]) < 5 and abs(y - L["y"]) < 5:
                return None
            L["active"] = True
            L["lines"] = [tk.Frame(t, bg=self.palette["accent"]) for _ in range(4)]
            self._lasso_tick()
        self._lasso_update()
        return "break"

    def _lasso_tick(self):
        """Auto-scroll while the pointer is held near the top/bottom edge."""
        L = self._lasso
        if not L or not L["active"]:
            return
        t = self.tree
        _first, y0, _rh = self._row_geom()
        top = y0 if y0 is not None else 24
        y, h = L["my"], t.winfo_height()
        if y < top + 6:
            t.yview_scroll(-1 - int(max(0, top - y) // 25), "units")
        elif y > h - 8:
            t.yview_scroll(1 + int(max(0, y - h) // 25), "units")
        self._lasso_update()
        L["job"] = self.root.after(60, self._lasso_tick)

    def _lasso_update(self):
        L = self._lasso
        if not L or not L["active"]:
            return
        t = self.tree
        n = len(self.shown)
        first, y0, rh = self._row_geom()
        if y0 is None or not n:
            return
        x, y = L["mx"], L["my"]
        ax, ay = L["x"], y0 + (L["frow"] - first) * rh
        cur_row = first + (y - y0) / rh
        lo, hi = sorted((L["frow"], cur_row))
        i0 = max(0, int(lo // 1))
        i1 = min(n - 1, int(hi // 1))
        key = (i0, i1)
        if key != L["last"]:
            L["last"] = key
            ids = {str(i) for i in range(i0, i1 + 1)} if i0 <= i1 else set()
            self._set_selection(L["base"] | ids if L["extend"] else ids)
            t.focus(str(max(0, min(n - 1, int(cur_row // 1)))))

        # draw the blue outline (edges that are scrolled out of view are hidden)
        w, h = t.winfo_width(), t.winfo_height()
        rx1, rx2 = sorted((ax, x))
        ry1, ry2 = sorted((ay, y))
        cx1, cx2 = [int(max(0, min(w - 1, v))) for v in (rx1, rx2)]
        cy1, cy2 = [int(max(0, min(h - 1, v))) for v in (ry1, ry2)]
        top, bottom, left, right = L["lines"]
        wd, ht = cx2 - cx1 + 1, cy2 - cy1 + 1
        specs = ((top, 0 <= ry1 <= h - 1, dict(x=cx1, y=cy1, width=wd, height=1)),
                 (bottom, 0 <= ry2 <= h - 1, dict(x=cx1, y=cy2, width=wd, height=1)),
                 (left, 0 <= rx1 <= w - 1, dict(x=cx1, y=cy1, width=1, height=ht)),
                 (right, 0 <= rx2 <= w - 1, dict(x=cx2, y=cy1, width=1, height=ht)))
        for line, visible, geo in specs:
            if visible:
                line.place(**geo)
                line.lift()
            else:
                line.place_forget()

    def lasso_release(self, event):
        L = self._lasso
        self._lasso = None
        if not L:
            return None
        if L["job"]:
            try:
                self.root.after_cancel(L["job"])
            except tk.TclError:
                pass
        for f in L["lines"] or []:
            f.destroy()
        if L["active"]:
            self.update_status()
            return "break"
        return None

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

    # ------------------------------------------------------- sidebar + usage
    def quick_entries(self):
        """[(label, path, kind)] shown in Quick access (defaults + your pins)."""
        hidden = set(self.cfg.get("unpinned", []))
        out = [(n, p, ("default", n)) for n, p in self.quick if n not in hidden]
        for p in self.cfg.get("pins", []):
            if p in VIRTUAL:
                out.append((self.display_path(p), p, ("pin", p)))
            elif os.path.isdir(p):
                out.append((os.path.basename(p.rstrip("/\\")) or p, p, ("pin", p)))
        return out

    def build_sidebar(self):
        sb = self.sidebar
        sb.delete(*sb.get_children())
        self.side_paths.clear()
        self.side_kind.clear()
        self.drive_rows = []

        qa = sb.insert("", "end", "qa_head", text="QUICK ACCESS", open=True,
                       tags=("head",))
        for label, p, kind in self.quick_entries():
            icon = QUICK_ICONS.get(label, "📌" if kind[0] == "pin" else "📁")
            if kind[0] == "pin":
                icon = "📌"
            iid = sb.insert(qa, "end", text=f"{icon}  {label}")
            self.side_paths[iid] = p
            self.side_kind[iid] = kind

        pc = sb.insert("", "end", "pc_head", text="THIS PC", open=True,
                       tags=("head",))
        self.side_paths[pc] = THIS_PC
        for d in scan_drives(quick=True):
            name = f"💾  {d.name}"
            iid = sb.insert(pc, "end", text=name)
            self.side_paths[iid] = d.path
            self.drive_rows.append((iid, name, d.path))
        self.apply_usage_text()          # cached numbers right away
        self.refresh_usage(force=True)   # then fetch fresh ones in the background
        self.update_pin_button()

    def apply_usage_text(self):
        for iid, name, path in self.drive_rows:
            if not self.sidebar.exists(iid):
                continue
            u = self.usage.get(path)
            text = name if not u else f"{name}  ·  {human_size(u[0])} / {human_size(u[1])}"
            self.sidebar.item(iid, text=text)

    def refresh_usage(self, force=False):
        """Read drive usage on worker threads (slow drives never freeze the UI)."""
        now = time.time()
        if not self.drive_rows:
            return
        if self._usage_busy and now - self._usage_last < 30:
            return
        if not force and now - self._usage_last < 5:
            return
        self._usage_last = now
        self._usage_busy = True
        results = {}

        def probe(p):
            try:
                total, used, _free = shutil.disk_usage(p)
                results[p] = (used, total)
            except OSError:
                pass

        threads = [threading.Thread(target=probe, args=(p,), daemon=True)
                   for _i, _n, p in self.drive_rows]
        for th in threads:
            th.start()
        self._poll_usage(results, threads, now + 20)

    def _poll_usage(self, results, threads, deadline):
        self.usage.update(dict(results))
        self.apply_usage_text()
        if any(th.is_alive() for th in threads) and time.time() < deadline:
            self.root.after(250, lambda: self._poll_usage(results, threads, deadline))
        else:
            self._usage_busy = False

    # --------------------------------------------------------------- navigate
    @staticmethod
    def display_path(target):
        if target == THIS_PC:
            return "This PC"
        if target == RECYCLE:
            return "Recycle Bin"
        return target

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
        self.path_var.set(self.display_path(target))
        self.update_pin_button()
        self.save_tab()
        self.refresh_tabs()
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
        self.refresh_usage()   # throttled; keeps the sidebar numbers fresh

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
        if not curr.isdigit() or int(curr) >= len(self.shown):
            return
        item = self.shown[int(curr)]
        if item.is_dir:
            self.navigate(item.path)
        elif not dirs_only:
            if self.cwd == RECYCLE:
                self.restore_selected()
            else:
                self.open_file(item.path)

    def open_file(self, path):
        """Open with the app saved via 'Always use this app', else the default."""
        ext = os.path.splitext(path)[1].lower()
        saved = self.cfg.get("assoc", {}).get(ext) if ext else None
        if saved:
            try:
                launch_with(saved["argv"], path)
                return
            except Exception as err:
                self.flash(f"{saved['name']} failed ({err}); using the default app")
        try:
            default_open(path)
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
        self.path_var.set(self.display_path(self.cwd))
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
    # Every tab keeps its own folder, history, sort, filter and selection.
    # `save_tab` copies the live view into the current tab; `load_tab` puts a
    # tab's saved state back on screen. All switching goes through these two.
    def tab_title(self, t):
        if not t.cwd:
            return "New tab"
        if t.cwd in VIRTUAL:
            return self.display_path(t.cwd)
        return loc_basename(t.cwd.rstrip("/\\")) or t.cwd

    def save_tab(self):
        if not (0 <= self.cur < len(self.tabs)):
            return
        t = self.tabs[self.cur]
        t.cwd = self.cwd
        t.back = self.back
        t.fwd = self.fwd
        t.sort_col = self.sort_col
        t.sort_reverse = self.sort_reverse
        t.filter = self.filter_var.get()
        t.sel = [i.path for i in self.selected_items()]
        foc = self.tree.focus()
        t.focus = (self.shown[int(foc)].path
                   if foc.isdigit() and int(foc) < len(self.shown) else None)

    def load_tab(self, t):
        if not t.cwd or not exists_loc(t.cwd):
            t.cwd = os.path.expanduser("~")
        sel, foc = list(t.sel), t.focus      # navigate() overwrites them
        self.back, self.fwd = t.back, t.fwd
        self.sort_col, self.sort_reverse = t.sort_col, t.sort_reverse
        self._suppress_filter = True
        self.filter_var.set(t.filter)
        self._suppress_filter = False
        self.cwd = t.cwd
        self.navigate(t.cwd, record=False)
        self.restore_view(sel, foc)
        self.tree.focus_set()

    def restore_view(self, sel, foc):
        index = {it.path: i for i, it in enumerate(self.shown)}
        ids = [str(index[p]) for p in sel if p in index]
        if ids:
            self.tree.selection_set(ids)
        target = str(index[foc]) if foc in index else (ids[0] if ids else None)
        if target is not None:
            self.tree.focus(target)
            self.tree.see(target)
        self.update_status()

    def switch_tab(self, index):
        if not (0 <= index < len(self.tabs)) or index == self.cur:
            return
        self.save_tab()
        self.cur = index
        self.load_tab(self.tabs[index])

    def new_tab(self):
        self.save_tab()
        t = Tab(self.cwd or os.path.expanduser("~"))
        self.tabs.append(t)
        self.cur = len(self.tabs) - 1
        self.load_tab(t)

    def close_tab(self, index=None):
        if index is None:
            index = self.cur
        if len(self.tabs) <= 1 or not (0 <= index < len(self.tabs)):
            return
        if index == self.cur:
            del self.tabs[index]
            self.cur = min(index, len(self.tabs) - 1)
            self.load_tab(self.tabs[self.cur])
        else:
            self.save_tab()
            del self.tabs[index]
            if index < self.cur:
                self.cur -= 1
            self.refresh_tabs()

    def next_tab(self, step=1):
        if len(self.tabs) > 1:
            self.switch_tab((self.cur + step) % len(self.tabs))

    def goto_tab(self, index):
        self.switch_tab(index)

    def refresh_tabs(self):
        """Rebuild the tab strip: [title x] [title x] [+]."""
        P = self.palette
        bar = self.tabbar
        for w in bar.winfo_children():
            w.destroy()
        bar.configure(bg=P["bg"])
        n = len(self.tabs)
        maxc = max(6, min(22, 120 // max(n, 1)))
        later = self.root.after_idle  # act after this click finishes

        for i, t in enumerate(self.tabs):
            sel = i == self.cur
            bg = P["tab_sel"] if sel else P["bg"]
            fg = P["fg"] if sel else P["dim"]
            title = self.tab_title(t)
            if len(title) > maxc:
                title = title[:maxc - 1] + "…"
            f = tk.Frame(bar, bg=bg, cursor="hand2")
            f.pack(side="left", padx=(0, 3))
            row = tk.Frame(f, bg=bg)
            row.pack(side="top")
            lab = tk.Label(row, text=title, bg=bg, fg=fg, padx=12, pady=6,
                           cursor="hand2")
            lab.pack(side="left")
            ul = tk.Frame(f, height=2, bg=P["accent"] if sel else bg)
            ul.pack(side="top", fill="x")
            if n > 1:
                x = tk.Label(row, text="×", bg=bg, fg=P["dim"], padx=6,
                             font=self.big_font, cursor="hand2")
                x.pack(side="left", padx=(0, 4))
                x.bind("<Enter>", lambda e, x=x: x.configure(
                    bg=P["danger"], fg="#ffffff"))
                x.bind("<Leave>", lambda e, x=x, bg=bg: x.configure(
                    bg=bg, fg=P["dim"]))
                x.bind("<Button-1>", lambda e, i=i: later(self.close_tab, i))
            for w in (f, row, lab, ul):
                w.bind("<Button-1>", lambda e, i=i: later(self.switch_tab, i))
                w.bind("<Button-2>", lambda e, i=i: later(self.close_tab, i))
            if not sel:
                def paint(color, ws=(f, row, lab, ul)):
                    for w in ws:
                        w.configure(bg=color)
                for w in (f, row, lab):
                    w.bind("<Enter>", lambda e, p=paint: p(P["hover"]))
                    w.bind("<Leave>", lambda e, p=paint, bg=bg: p(bg))

        plus = tk.Label(bar, text="+", bg=P["bg"], fg=P["fg"], padx=12,
                        pady=2, font=self.big_font, cursor="hand2")
        plus.pack(side="left", padx=(2, 0))
        plus.bind("<Enter>", lambda e: plus.configure(bg=P["hover"]))
        plus.bind("<Leave>", lambda e: plus.configure(bg=P["bg"]))
        plus.bind("<Button-1>", lambda e: later(self.new_tab))

        # keep the strip a constant height so the page below never shifts
        bar.update_idletasks()
        need = max([c.winfo_reqheight() for c in bar.winfo_children()] + [0])
        self._tab_h = max(getattr(self, "_tab_h", 0), need)
        bar.pack_propagate(False)
        bar.configure(height=self._tab_h)

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

    def is_pinned(self, path):
        return any(same_loc(p, path) for _l, p, _k in self.quick_entries())

    def update_pin_button(self):
        b = getattr(self, "btn_pin", None)
        if b is None or not self.cwd:
            return
        on = self.is_pinned(self.cwd)
        # same glyph either way (☆ vs ★ can come from different fonts with
        # different heights, which made the whole toolbar twitch)
        b.configure(text="★",
                    fg=self.palette["accent"] if on else self.palette["dim"])

    def pin_path(self, path):
        if path not in VIRTUAL and not os.path.isdir(path):
            return
        # a built-in entry you removed simply comes back
        for label, p in self.quick:
            if same_loc(p, path):
                gone = self.cfg.setdefault("unpinned", [])
                if label in gone:
                    gone.remove(label)
                    save_config(self.cfg)
                    self.build_sidebar()
                    self.flash(f"Pinned: {label}")
                else:
                    self.flash("Already in Quick access")
                return
        pins = self.cfg.setdefault("pins", [])
        if any(same_loc(p, path) for p in pins):
            self.flash("Already in Quick access")
            return
        pins.append(path)
        save_config(self.cfg)
        self.build_sidebar()
        self.flash(f"Pinned: {self.display_path(path)}")

    def unpin_path(self, path):
        changed = False
        pins = self.cfg.get("pins", [])
        for p in list(pins):
            if same_loc(p, path):
                pins.remove(p)
                changed = True
        gone = self.cfg.setdefault("unpinned", [])
        for label, p in self.quick:
            if same_loc(p, path) and label not in gone:
                gone.append(label)
                changed = True
        if changed:
            save_config(self.cfg)
            self.build_sidebar()
            self.flash("Removed from Quick access")
        else:
            self.flash("Not in Quick access")

    def pin_current(self):
        self.pin_path(self.cwd)

    def unpin_current(self):
        self.unpin_path(self.cwd)

    def toggle_pin_current(self):
        if self.is_pinned(self.cwd):
            self.unpin_path(self.cwd)
        else:
            self.pin_path(self.cwd)

    def restore_quick_defaults(self):
        self.cfg["unpinned"] = []
        save_config(self.cfg)
        self.build_sidebar()
        self.flash("Default Quick access folders restored")

    def unpin_side_item(self, iid):
        kind = self.side_kind.get(iid)
        if not kind:
            return
        if kind[0] == "default":
            gone = self.cfg.setdefault("unpinned", [])
            if kind[1] not in gone:
                gone.append(kind[1])
        else:
            pins = self.cfg.get("pins", [])
            if kind[1] in pins:
                pins.remove(kind[1])
        save_config(self.cfg)
        self.build_sidebar()
        self.flash("Removed from Quick access")

    def unpin_side(self):
        self.unpin_side_item(self.sidebar.focus())

    def on_side_context(self, event):
        item = self.sidebar.identify_row(event.y)
        m = self.new_menu(self.sidebar)
        if item == "qa_head":
            self.ma(m, "Pin current folder", lambda: self.pin_path(self.cwd))
            self.ma(m, "Restore default folders", self.restore_quick_defaults)
        elif item in self.side_paths:
            self.sidebar.focus(item)
            self.sidebar.selection_set(item)
            p = self.side_paths[item]
            self.ma(m, "Open", lambda: self.navigate(p))
            if item in self.side_kind:
                self.ma(m, "Unpin from Quick access",
                        lambda: self.unpin_side_item(item))
            elif not self.is_pinned(p):
                self.ma(m, "Pin to Quick access", lambda: self.pin_path(p))
        else:
            return
        try:
            m.tk_popup(event.x_root, event.y_root)
        finally:
            m.grab_release()

    # --------------------------------------------------- Clipboard & Operations
    # --------------------------------------------------- Clipboard & Operations
    def selected_items(self):
        sel = self.tree.selection()
        return [self.shown[int(i)] for i in sel
                if i.isdigit() and int(i) < len(self.shown)]

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
        if not items or self.cwd == THIS_PC:
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
        dialog.configure(background=self.palette["bg"])
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
        ttk.Button(dialog, text="OK", command=submit, style="Accent.TButton").pack(side="right", padx=10, pady=10)

    def new_entry(self, is_dir=False):
        if self.cwd in VIRTUAL:
            return
        kind = "folder" if is_dir else "file"
        dialog = tk.Toplevel(self.root)
        dialog.title(f"New {kind}")
        dialog.geometry("360x120")
        dialog.transient(self.root)
        dialog.configure(background=self.palette["bg"])
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
        ttk.Button(dialog, text="OK", command=submit, style="Accent.TButton").pack(side="right", padx=10, pady=10)

    def properties_sel(self):
        items = self.selected_items()
        if not items:
            return
        item = items[0]
        p_win = tk.Toplevel(self.root)
        p_win.title("Properties")
        p_win.geometry("340x260")
        p_win.transient(self.root)
        p_win.configure(background=self.palette["bg"])

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
        ma = self.ma
        real = self.cwd not in VIRTUAL
        if self.cwd == RECYCLE:
            if items:
                ma(m, "Restore", self.restore_selected, "restore")
                ma(m, "Delete permanently",
                   lambda: self.delete_sel(permanent=True), "delete_perm")
            ma(m, "Empty Recycle Bin", self.empty_bin, "empty_bin")
        else:
            if items:
                item = items[0]
                ma(m, "Open", self.open_focused, "open")
                if len(items) == 1 and not item.is_dir and real:
                    ma(m, "Open with…", lambda p=item.path: self.open_with(p),
                       "open_with")
                m.add_separator()
                if real:
                    ma(m, "Cut", self.funcs["cut"], "cut")
                    ma(m, "Copy", self.funcs["copy"], "copy")
                ma(m, "Copy path", self.copy_path, "copy_path")
                if len(items) == 1 and item.is_dir:
                    if self.is_pinned(item.path):
                        ma(m, "Unpin from Quick access",
                           lambda p=item.path: self.unpin_path(p))
                    else:
                        ma(m, "Pin to Quick access",
                           lambda p=item.path: self.pin_path(p))
                if real:
                    ma(m, "Rename", self.rename, "rename")
                    ma(m, "Delete", self.delete_sel, "delete")
                ma(m, "Properties", self.properties_sel, "properties")
                m.add_separator()
            if real:
                if self.clip:
                    ma(m, "Paste", self.paste, "paste")
                ma(m, "New folder", self.funcs["new_folder"], "new_folder")
                ma(m, "New file", self.funcs["new_file"], "new_file")
                m.add_separator()
            pinned = self.is_pinned(self.cwd)
            ma(m, "Unpin this location" if pinned
               else "Pin this location to Quick access",
               self.toggle_pin_current, "pin")
            ma(m, "Refresh", self.reload, "refresh")
        try:
            m.tk_popup(x, y)
        finally:
            m.grab_release()

    # ---------------------------------------------------------------- Open with
    def open_with_sel(self):
        items = self.selected_items()
        if len(items) == 1 and not items[0].is_dir and self.cwd not in VIRTUAL:
            self.open_with(items[0].path)

    def open_with(self, path):
        """Dialog: search every installed app, pick one, optionally remember it."""
        P = self.palette
        name = os.path.basename(path)
        ext = os.path.splitext(name)[1].lower()
        assoc = self.cfg.setdefault("assoc", {})
        saved = assoc.get(ext) if ext else None

        dlg = tk.Toplevel(self.root)
        dlg.title("Open with")
        dlg.geometry("460x540")
        dlg.minsize(380, 400)
        dlg.transient(self.root)
        dlg.configure(background=P["bg"])

        ttk.Label(dlg, text=f"Open “{name}” with:",
                  padding=(10, 10, 10, 4)).pack(anchor="w")
        fvar = tk.StringVar()
        fent = ttk.Entry(dlg, textvariable=fvar)
        fent.pack(fill="x", padx=10)

        box = ttk.Frame(dlg)
        box.pack(fill="both", expand=True, padx=10, pady=6)
        lst = ttk.Treeview(box, show="tree", selectmode="browse")
        lst.column("#0", stretch=True)
        vs = ttk.Scrollbar(box, orient="vertical", command=lst.yview)
        lst.configure(yscrollcommand=vs.set)
        lst.pack(side="left", fill="both", expand=True)
        vs.pack(side="right", fill="y")

        info = tk.StringVar(master=dlg)
        ttk.Label(dlg, textvariable=info, padding=(10, 0)).pack(anchor="w")

        always = tk.BooleanVar(master=dlg, value=False)
        if ext:
            text = f"Always use this app to open {ext} files"
        else:
            text = "Always use this app (needs a file extension)"
        chk = ttk.Checkbutton(dlg, text=text, variable=always)
        chk.pack(anchor="w", padx=10, pady=(6, 0))
        if not ext:
            chk.state(["disabled"])

        btns = ttk.Frame(dlg, padding=10)
        btns.pack(fill="x")
        visible = []

        def populate(first=False):
            lst.delete(*lst.get_children())
            visible.clear()
            apps = self._apps
            if apps is None:
                info.set("Loading installed apps…")
                return
            terms = fvar.get().lower().split()
            for a in apps:
                low = a["name"].lower()
                if all(t in low for t in terms):
                    visible.append(a)
            for i, a in enumerate(visible):
                lst.insert("", "end", iid=str(i), text="  " + a["name"])
            note = f"{len(visible)} of {len(apps)} apps"
            if saved and first:
                note += f"  ·  saved for {ext}: {saved['name']}"
            info.set(note)
            if visible:
                pick = 0
                if first and saved:
                    for i, a in enumerate(visible):
                        if a["name"] == saved["name"]:
                            pick = i
                            break
                lst.selection_set(str(pick))
                lst.focus(str(pick))
                lst.see(str(pick))

        def wait_apps():
            if not dlg.winfo_exists():
                return
            if self._apps is None:
                dlg.after(200, wait_apps)
            else:
                populate(first=True)

        def current_app():
            sel = lst.selection()
            if sel and sel[0].isdigit() and int(sel[0]) < len(visible):
                return visible[int(sel[0])]
            return None

        def run(app):
            try:
                launch_with(app["argv"], path)
            except Exception as err:
                messagebox.showerror("Open with", f"Could not start {app['name']}:\n{err}",
                                     parent=dlg)
                return
            if always.get() and ext:
                assoc[ext] = {"name": app["name"], "argv": list(app["argv"])}
                save_config(self.cfg)
                self.flash(f"{app['name']} will now open {ext} files")
            dlg.destroy()
            self.tree.focus_set()

        def do_open(event=None):
            app = current_app()
            if app:
                run(app)
            return "break"

        def browse():
            if IS_WIN:
                types = [("Programs", "*.exe *.bat *.cmd"), ("All files", "*.*")]
            else:
                types = [("All files", "*")]
            fn = filedialog.askopenfilename(parent=dlg, title="Choose an application",
                                            filetypes=types)
            if fn:
                run(app_from_file(fn))

        def move_sel(delta):
            if not visible:
                return "break"
            sel = lst.selection()
            idx = int(sel[0]) if sel and sel[0].isdigit() else -1
            idx = max(0, min(len(visible) - 1, idx + delta))
            lst.selection_set(str(idx))
            lst.focus(str(idx))
            lst.see(str(idx))
            return "break"

        def forget():
            assoc.pop(ext, None)
            save_config(self.cfg)
            self.flash(f"Cleared the saved app for {ext} files")
            dlg.destroy()

        def win_chooser():
            try:
                os.startfile(path, "openas")
                dlg.destroy()
            except Exception as err:
                messagebox.showerror("Open with", str(err), parent=dlg)

        ttk.Button(btns, text="Browse…", command=browse).pack(side="left")
        if IS_WIN:
            ttk.Button(btns, text="Windows chooser…",
                       command=win_chooser).pack(side="left", padx=(6, 0))
        if saved:
            ttk.Button(btns, text="Forget saved app",
                       command=forget).pack(side="left", padx=(6, 0))
        ttk.Button(btns, text="Open", command=do_open, style="Accent.TButton").pack(side="right")
        ttk.Button(btns, text="Cancel",
                   command=dlg.destroy).pack(side="right", padx=(0, 6))

        fvar.trace_add("write", lambda *a: populate())
        fent.bind("<Return>", do_open)
        fent.bind("<Down>", lambda e: move_sel(1))
        fent.bind("<Up>", lambda e: move_sel(-1))
        lst.bind("<Return>", do_open)
        lst.bind("<Double-1>", do_open)
        dlg.bind("<Escape>", lambda e: dlg.destroy())

        fent.focus_set()
        populate(first=True)
        if self._apps is None:
            wait_apps()
        try:
            dlg.wait_visibility()
            dlg.grab_set()
        except tk.TclError:
            pass

    def capture_key(self, parent):
        """Small modal: press a key combination, get it back as 'Ctrl+Shift+T'."""
        P = self.palette
        dlg = tk.Toplevel(parent)
        dlg.title("New shortcut")
        dlg.geometry("380x190")
        dlg.transient(parent)
        dlg.configure(background=P["bg"])
        state = {"spec": None, "result": None}
        shown = tk.StringVar(master=dlg, value="…")
        ttk.Label(dlg, text="Press the key combination you want",
                  padding=(16, 18, 16, 4)).pack()
        ttk.Label(dlg, textvariable=shown, font=self.big_font,
                  padding=(16, 8)).pack()
        ttk.Label(dlg, text="Esc cancels", style="Dim.TLabel").pack()
        row = ttk.Frame(dlg, padding=14)
        row.pack(side="bottom", fill="x")
        ok = ttk.Button(row, text="Use this shortcut", style="Accent.TButton",
                        takefocus=False)
        ok.state(["disabled"])
        ok.pack(side="right")
        ttk.Button(row, text="Cancel", command=dlg.destroy,
                   takefocus=False).pack(side="right", padx=(0, 6))

        def accept():
            state["result"] = state["spec"]
            dlg.destroy()

        ok.configure(command=accept)

        def on_key(e):
            if e.keysym == "Escape":
                dlg.destroy()
                return "break"
            spec = event_to_spec(e)
            if spec:
                state["spec"] = spec
                shown.set(spec)
                ok.state(["!disabled"])
            return "break"

        dlg.bind("<KeyPress>", on_key)
        dlg.focus_force()
        try:
            dlg.wait_visibility()
            dlg.grab_set()
        except tk.TclError:
            pass
        dlg.wait_window()
        return state["result"]

    def assign_key(self, aid, spec, parent):
        spec = norm_spec(spec)
        mods, key = split_key(spec)
        if not spec:
            return False
        if spec in RESERVED:
            messagebox.showinfo("Shortcut", f"{spec} is reserved and can't be reassigned.",
                                parent=parent)
            return False
        if not ({"Ctrl", "Alt"} & mods) and (len(key) == 1 or key == "Space"):
            messagebox.showinfo(
                "Shortcut",
                "Letters, digits and Space need Ctrl or Alt, otherwise they "
                "would break type-to-select.", parent=parent)
            return False
        if spec in self.keys_for(aid):
            return True
        owner = next((o for o in ACTION_INFO
                      if o != aid and spec in self.keys_for(o)), None)
        if owner:
            if not messagebox.askyesno(
                    "Shortcut in use",
                    f"{spec} is already used by “{ACTION_INFO[owner][1]}”.\n\n"
                    f"Move it to “{ACTION_INFO[aid][1]}”?", parent=parent):
                return False
            self.set_keys(owner, [k for k in self.keys_for(owner) if k != spec])
        self.set_keys(aid, self.keys_for(aid) + [spec])
        self.flash(f"{spec} → {ACTION_INFO[aid][1]}")
        return True

    def show_shortcuts(self):
        """Window listing every shortcut, with add / remove / reset."""
        old = getattr(self, "_sc_win", None)
        if old is not None:
            try:
                if old.winfo_exists():
                    old.deiconify()
                    old.lift()
                    old.focus_force()
                    return
            except tk.TclError:
                pass
        P = self.palette
        win = tk.Toplevel(self.root)
        self._sc_win = win
        win.title("Keyboard shortcuts")
        win.geometry("740x660")
        win.minsize(580, 480)
        win.transient(self.root)
        win.configure(background=P["bg"])

        ttk.Label(win, text="Keyboard shortcuts", font=self.big_font,
                  padding=(16, 14, 16, 0)).pack(anchor="w")
        ttk.Label(win, text="Pick an action, then add or remove its shortcuts. "
                  "An action can have several.", style="Dim.TLabel",
                  padding=(16, 2, 16, 8)).pack(anchor="w")
        fvar = tk.StringVar(master=win)
        ent = ttk.Entry(win, textvariable=fvar)
        ent.pack(fill="x", padx=16, pady=(0, 8))

        box = ttk.Frame(win)
        box.pack(fill="both", expand=True, padx=16)
        tv = ttk.Treeview(box, columns=("keys",), selectmode="browse")
        tv.heading("#0", text="Action", anchor="w")
        tv.heading("keys", text="Shortcuts", anchor="w")
        tv.column("#0", width=300, stretch=True)
        tv.column("keys", width=320, stretch=True)
        vs = ttk.Scrollbar(box, orient="vertical", command=tv.yview)
        tv.configure(yscrollcommand=vs.set)
        tv.pack(side="left", fill="both", expand=True)
        vs.pack(side="right", fill="y")
        tv.tag_configure("cat", foreground=P["dim"], font=self.small_bold)

        detail = ttk.Frame(win, padding=(16, 10, 16, 0))
        detail.pack(fill="x")
        title_var = tk.StringVar(master=win, value="Select an action above.")
        ttk.Label(detail, textvariable=title_var).pack(anchor="w")
        lb = tk.Listbox(detail, height=3, activestyle="none",
                        exportselection=False, relief="flat", bd=0,
                        highlightthickness=1, bg=P["field"], fg=P["fg"],
                        selectbackground=P["sel_bg"],
                        selectforeground=P["sel_fg"],
                        highlightbackground=P["border"],
                        highlightcolor=P["accent"])
        lb.pack(fill="x", pady=(6, 0))

        btns = ttk.Frame(win, padding=(16, 10, 16, 14))
        btns.pack(fill="x")
        b_add = ttk.Button(btns, text="Add shortcut…", style="Accent.TButton",
                           takefocus=False)
        b_rm = ttk.Button(btns, text="Remove selected", takefocus=False)
        b_reset = ttk.Button(btns, text="Reset this action", takefocus=False)
        b_all = ttk.Button(btns, text="Reset all…", takefocus=False)
        ttk.Button(btns, text="Close", command=win.destroy,
                   takefocus=False).pack(side="right")
        for b in (b_add, b_rm, b_reset):
            b.pack(side="left", padx=(0, 6))
        b_all.pack(side="left", padx=(12, 0))

        def current():
            sel = tv.selection()
            return sel[0] if sel and sel[0] in ACTION_INFO else None

        def refresh_detail():
            aid = current()
            lb.delete(0, "end")
            for b in (b_add, b_reset):
                b.state(["!disabled" if aid else "disabled"])
            if not aid:
                b_rm.state(["disabled"])
                sel = tv.selection()
                if sel and sel[0].startswith("fixed:"):
                    title_var.set("This is a built-in key and can't be changed.")
                else:
                    title_var.set("Select an action above.")
                return
            title_var.set(f"Shortcuts for “{ACTION_INFO[aid][1]}”")
            keys = self.keys_for(aid)
            for k in keys:
                lb.insert("end", k)
            if keys:
                lb.selection_set(0)
            b_rm.state(["!disabled" if keys else "disabled"])

        def populate(select=None):
            keep = select or (tv.selection()[0] if tv.selection() else None)
            tv.delete(*tv.get_children())
            terms = fvar.get().lower().split()
            cats = {}
            for aid, label, cat, _d, _s in ACTION_DEFS:
                keys = self.keys_for(aid)
                hay = " ".join([label, cat] + keys).lower()
                if all(t in hay for t in terms):
                    cats.setdefault(cat, []).append((aid, label, keys))
            for cat, rows in cats.items():
                cid = tv.insert("", "end", iid="cat:" + cat, text=cat.upper(),
                                open=True, tags=("cat",))
                for aid, label, keys in rows:
                    tv.insert(cid, "end", iid=aid, text="   " + label,
                              values=("   ·   ".join(keys) or "—",))
            fixed = [(i, k, d) for i, (k, d) in enumerate(FIXED_KEYS)
                     if all(t in (k + " " + d + " built-in").lower() for t in terms)]
            if fixed:
                cid = tv.insert("", "end", iid="cat:fixed",
                                text="BUILT-IN (CAN'T BE CHANGED)", open=True,
                                tags=("cat",))
                for i, k, d in fixed:
                    tv.insert(cid, "end", iid=f"fixed:{i}", text="   " + d,
                              values=(k,))
            if keep and tv.exists(keep):
                tv.selection_set(keep)
                tv.see(keep)
            refresh_detail()

        def add_key():
            aid = current()
            if not aid:
                return
            spec = self.capture_key(win)
            if spec and self.assign_key(aid, spec, win):
                populate(aid)

        def remove_key():
            aid = current()
            sel = lb.curselection()
            if not aid or not sel:
                return
            keys = self.keys_for(aid)
            if sel[0] < len(keys):
                del keys[sel[0]]
                self.set_keys(aid, keys)
                populate(aid)

        def reset_one():
            aid = current()
            if aid:
                self.cfg.get("keys", {}).pop(aid, None)
                save_config(self.cfg)
                self.apply_bindings()
                self.fill_menus()
                populate(aid)

        def reset_all():
            if self.reset_all_keys(win):
                populate()

        b_add.configure(command=add_key)
        b_rm.configure(command=remove_key)
        b_reset.configure(command=reset_one)
        b_all.configure(command=reset_all)
        tv.bind("<<TreeviewSelect>>", lambda e: refresh_detail())
        lb.bind("<Delete>", lambda e: remove_key())
        fvar.trace_add("write", lambda *a: populate())
        win.bind("<Escape>", lambda e: win.destroy())
        populate()
        ent.focus_set()

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
