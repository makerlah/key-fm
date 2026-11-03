"""Discover installed apps and launch files with them."""
import os
import re
import shlex
import shutil
import subprocess
import sys
import traceback

from .constants import IS_WIN  # #35: single source of truth


def parse_desktop(path):
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
                         "Terminal", "TryExec", "Icon", "Path",
                         "OnlyShowIn", "NotShowIn") and k not in data:
                    data[k] = v.strip()
    except OSError:
        return None
    if data.get("Type") != "Application" or not data.get("Exec") or not data.get("Name"):
        return None
    if any(data.get(k, "").lower() == "true" for k in ("NoDisplay", "Hidden", "Terminal")):
        return None
    # #8: TryExec
    try_exec = data.get("TryExec")
    if try_exec and not (os.path.isfile(try_exec) or shutil.which(try_exec)):
        return None
    # #9: OnlyShowIn / NotShowIn (only filter if we actually know the DE)
    desktops_env = os.environ.get("XDG_CURRENT_DESKTOP") or ""
    desktops_set = {d for d in desktops_env.split(":") if d}
    if desktops_set:
        only = data.get("OnlyShowIn")
        if only:
            allowed = {s for s in only.split(";") if s}
            if allowed and not (allowed & desktops_set):
                return None
        not_show = data.get("NotShowIn")
        if not_show:
            denied = {s for s in not_show.split(";") if s}
            if denied & desktops_set:
                return None
    try:
        parts = shlex.split(data["Exec"])
    except ValueError:
        return None

    name = data["Name"]
    icon = data.get("Icon", "")
    args, has_file = [], False
    for p in parts:
        q = p.replace("%%", "\0")
        if re.search(r"%[fFuU]", q):
            has_file = True
            q = re.sub(r"%[fFuU]", "{file}", q)
        # #3: %c/%C -> translated app name (single arg)
        q = re.sub(r"%[cC]", lambda _m: name, q)
        # #3: %i -> "--icon <Icon>" (two args, spec says standalone)
        if q == "%i":
            if icon:
                args.append("--icon")
                args.append(icon)
            continue
        q = re.sub(r"%i", "", q)     # embedded %i: drop
        q = re.sub(r"%[a-zA-Z]", "", q)
        q = q.replace("\0", "%")
        # #17: drop literal empty args too
        if q == "":
            continue
        args.append(q)
    if not args:
        return None
    if not has_file:
        args.append("{file}")
    result = {"name": name, "argv": args}
    # #10: Path= (working directory)
    if data.get("Path"):
        result["cwd"] = data["Path"]
    return result


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
                # #1: only mark seen on successful parse
                app = parse_desktop(os.path.join(root_, fn))
                if app:
                    seen.add(fn)
                    apps.append(app)
    return apps


def mac_apps():
    home = os.path.expanduser("~")
    apps = []
    roots = ("/Applications", "/System/Applications",
             os.path.join(home, "Applications"))
    for d in roots:
        try:
            entries = list(os.scandir(d))
        except OSError:
            continue
        for entry in entries:
            try:
                if entry.name.endswith(".app"):
                    apps.append({"name": entry.name[:-4],
                                 "argv": ["open", "-a", entry.path, "{file}"]})
                elif entry.is_dir(follow_symlinks=False):
                    # #19: 1-level recursion (e.g. /Applications/Utilities/*.app)
                    try:
                        for sub in os.scandir(entry.path):
                            if sub.name.endswith(".app"):
                                apps.append({"name": sub.name[:-4],
                                             "argv": ["open", "-a", sub.path, "{file}"]})
                    except OSError:
                        pass
            except OSError:
                continue
    return apps


_APPS_CACHE = None  # #6: cache discovery (only refreshed via force=True)


def win_apps():
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
    # #6: 10 s timeout + skip PS if not present
    if dirs and shutil.which("powershell"):
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
                capture_output=True, timeout=10,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)).stdout
            rows = []
            for line in out.decode("utf-8", "replace").splitlines():
                name, _, target = line.partition("|")
                if name.strip() and target.strip():
                    rows.append((name.strip(), target.strip()))
            for name, target in sorted(rows, key=lambda r: r[0].lower()):
                add(name, target)
        except Exception:
            traceback.print_exc()  # #7

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
                    raw = os.path.expandvars(str(val)).strip()
                    if raw.startswith('"'):
                        end = raw.find('"', 1)
                        exe = raw[1:end] if end > 0 else raw.strip('"')
                    else:
                        exe = raw.split('"', 1)[0].split(' --', 1)[0].strip()
                    name = os.path.splitext(sub)[0]
                    add(name[:1].upper() + name[1:], exe)
            finally:
                winreg.CloseKey(key)
    except Exception:
        traceback.print_exc()

    # #36: 32-bit Python on 64-bit Windows → Sysnative (System32 gets redirected)
    sysroot = os.environ.get("SystemRoot", r"C:\Windows")
    sysdir = os.path.join(sysroot, "System32")
    if sys.maxsize < 2 ** 32:
        sysnative = os.path.join(sysroot, "Sysnative")
        if os.path.isdir(sysnative):
            sysdir = sysnative
    for name, exe in (("Notepad", "notepad.exe"), ("Paint", "mspaint.exe"),
                      ("WordPad", "write.exe")):
        add(name, os.path.join(sysdir, exe))
    if not any(a["name"] == "Notepad" for a in apps.values()):
        add("Notepad", os.path.join(sysdir, "notepad.exe"))
    return list(apps.values())


def list_apps(force=False):
    """Return discoverable apps. Cached; pass force=True to refresh (#6)."""
    global _APPS_CACHE
    if _APPS_CACHE is not None and not force:
        return list(_APPS_CACHE)
    try:
        if IS_WIN:
            apps = win_apps()
        elif sys.platform == "darwin":
            apps = mac_apps()
        else:
            apps = linux_apps()
    except Exception:
        traceback.print_exc()  # #7
        apps = []
    apps.sort(key=lambda a: a["name"].lower())
    _APPS_CACHE = apps
    return list(apps)


def app_from_file(fn):
    """Return an app dict to launch *fn* as a program, or None if unsupported.

    Windows can only Popen() real executables — batch/PowerShell scripts need
    an interpreter wrapper (#4). Anything else returns None so the caller can
    show a message and let the user pick something else.
    """
    fn = fn.rstrip("/\\")                 # #13
    basename = os.path.basename(fn) or fn
    # #14: strip double extensions before splitting the single one
    stripped = basename
    for ext in (".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst"):
        if stripped.lower().endswith(ext):
            stripped = stripped[:-len(ext)]
            break
    else:
        stripped = os.path.splitext(stripped)[0]
    base = stripped or basename
    if sys.platform == "darwin" and fn.endswith(".app"):
        return {"name": base, "argv": ["open", "-a", fn, "{file}"]}
    if IS_WIN:
        ext = os.path.splitext(fn)[1].lower()
        if ext in (".exe", ".com"):
            return {"name": base, "argv": [fn, "{file}"]}
        if ext in (".bat", ".cmd"):
            return {"name": base, "argv": ["cmd.exe", "/c", fn, "{file}"]}
        if ext == ".ps1":
            return {"name": base,
                    "argv": ["powershell.exe", "-NoProfile", "-File", fn, "{file}"]}
        return None          # #4: caller handles it
    return {"name": base, "argv": [fn, "{file}"]}


def launch_with(argv, path, cwd=None):
    """Launch argv with {file} replaced by *path*. Returns True/False (#15)."""
    if not argv:
        return False
    args = [a.replace("{file}", path) for a in argv]
    try:
        kwargs = {}
        if cwd:
            kwargs["cwd"] = cwd
        if IS_WIN:
            subprocess.Popen(args, **kwargs)
        else:
            subprocess.Popen(args, stdin=subprocess.DEVNULL,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                             start_new_session=True, **kwargs)
        return True
    except OSError:
        return False


def default_open(path):
    """Open *path* with the OS default handler. Returns True/False (#16)."""
    try:
        if IS_WIN:
            os.startfile(path)
        elif sys.platform == "darwin":
            subprocess.Popen(["open", path])
        else:
            xdg = shutil.which("xdg-open")
            if not xdg:
                return False
            subprocess.Popen([xdg, path], stdin=subprocess.DEVNULL,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                             start_new_session=True)
        return True
    except OSError:
        return False