"""Recycle Bin (Windows) and XDG / macOS trash handling."""
import os
import re
import shutil
import struct
import subprocess
import sys
from datetime import datetime
from urllib.parse import quote, unquote

from .constants import IS_WIN
from .helpers import remove_path, unique_path
from .drives import win_drive_roots
from .models import Item

try:
    from send2trash import send2trash
except Exception:
    send2trash = None

_SID = None


def win_sid():
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
        name = os.path.basename(orig) or n
        is_dir = os.path.isdir(rpath) and not os.path.islink(rpath)
        items.append(Item(name, rpath, is_dir, size, ts, False, "", orig, ipath))
    return items


def scan_trash():
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
                items.append(Item(n, rp,
                                  os.path.isdir(rp) and not os.path.islink(rp),
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
                items.append(Item(name, rp,
                                  os.path.isdir(rp) and not os.path.islink(rp),
                                  st.st_size, ts or st.st_mtime, False, "", orig,
                                  ip if os.path.exists(ip) else ""))
    except OSError:
        pass
    return items


def win_recycle(path):
    import ctypes
    from ctypes import wintypes
    path = os.path.abspath(path)

    class SHFILEOPSTRUCTW(ctypes.Structure):
        _pack_ = 4 if ctypes.sizeof(ctypes.c_void_p) == 4 else 8 
        _fields_ = [("hwnd", wintypes.HWND),
                    ("wFunc", wintypes.UINT),
                    ("pFrom", wintypes.LPCWSTR),
                    ("pTo", wintypes.LPCWSTR),
                    ("fFlags", ctypes.c_ushort),
                    ("fAnyOperationsAborted", ctypes.BOOL),
                    ("hNameMappings", ctypes.c_void_p),
                    ("lpszProgressTitle", wintypes.LPCWSTR)]

    buf = ctypes.create_unicode_buffer(path, len(path) + 2)
    op = SHFILEOPSTRUCTW()
    op.hwnd = None
    op.wFunc = 3
    op.pFrom = wintypes.LPCWSTR(ctypes.addressof(buf))
    op.pTo = None
    op.fFlags = 0x0040 | 0x0400
    res = ctypes.windll.shell32.SHFileOperationW(ctypes.byref(op))
    if op.fAnyOperationsAborted:
        raise OSError("cancelled")
    if res != 0 or os.path.lexists(path):
        raise OSError(f"could not move to the Recycle Bin (code {res})")


def win_empty_recycle_bin():
    import ctypes
    flags = 0x00000001 | 0x00000002 | 0x00000004
    res = ctypes.windll.shell32.SHEmptyRecycleBinW(None, None, flags)
    if res not in (0, 1):
        raise OSError(f"SHEmptyRecycleBinW failed (code {res})")


def move_to_trash(path):
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
            f.write("DeletionDate="
                    + datetime.now().strftime("%Y-%m-%dT%H:%M:%S") + "\n")
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