"""Pure helpers - formatting, path checks, clipboard, event wrapping."""
import os
import re
import shutil
import stat
import subprocess
import sys
from datetime import datetime

from .constants import IS_WIN, VIRTUAL


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


def loc_dirname(p):
    import ntpath
    return ntpath.dirname(p) if IS_WIN else os.path.dirname(p)


def loc_basename(p):
    import ntpath
    return ntpath.basename(p) if IS_WIN else os.path.basename(p)


def unique_path(folder, name, is_dir):
    """Return a non-existing path in `folder`, adding ' - copy [N]' if needed."""
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
                os.rmdir(path)
    else:
        if sys.version_info >= (3, 12):
            shutil.rmtree(path, onexc=_force_remove)
        else:
            shutil.rmtree(path, onerror=_force_remove)


def set_clipboard(root, text):
    """Put `text` on the system clipboard.

    On Linux, hand it to a clipboard manager so it survives app exit.
    """
    if sys.platform.startswith("linux"):
        data = text.encode("utf-8")
        for cmd in (["wl-copy"],
                    ["xclip", "-selection", "clipboard"],
                    ["xsel", "-b", "-i"]):
            if shutil.which(cmd[0]) is None:
                continue
            proc = None
            try:
                proc = subprocess.Popen(
                    cmd, stdin=subprocess.PIPE,
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    start_new_session=True)
                proc.communicate(data, timeout=2)
                if proc.returncode == 0:
                    return
            except (OSError, subprocess.TimeoutExpired):
                if proc is not None:
                    try: proc.kill()
                    except Exception: pass
    root.clipboard_clear()
    root.clipboard_append(text)
    root.update_idletasks()


def brk(fn):
    """Wrap a method as a Tk handler that stops further processing."""
    def handler(event=None):
        fn()
        return "break"
    return handler