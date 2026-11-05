"""Discover installed apps and launch files with them."""
import os
import re
import shlex
import subprocess
import sys

IS_WIN = sys.platform.startswith("win")


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
            continue
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
    base = os.path.splitext(os.path.basename(fn.rstrip("/\\")))[0] or fn
    if sys.platform == "darwin" and fn.rstrip("/").endswith(".app"):
        return {"name": base, "argv": ["open", "-a", fn, "{file}"]}
    return {"name": base, "argv": [fn, "{file}"]}


def launch_with(argv, path):
    args = [a.replace("{file}", path) for a in argv]
    if IS_WIN:
        subprocess.Popen(args)
    else:
        subprocess.Popen(args, stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         start_new_session=True)


def default_open(path):
    if IS_WIN:
        os.startfile(path)
    elif sys.platform == "darwin":
        subprocess.Popen(["open", path])
    else:
        subprocess.Popen(["xdg-open", path], stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         start_new_session=True)