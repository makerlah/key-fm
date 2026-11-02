"""Config load / save."""
import json
import os
import tempfile

from .constants import CONFIG_FILE, OLD_PIN_FILE
from .keys import ACTION_INFO, norm_spec


def _valid_assoc(v):
    # #2: reject empty name / empty argv so the launcher never sees bad data
    return (isinstance(v, dict)
            and isinstance(v.get("name"), str) and v["name"]
            and isinstance(v.get("argv"), list) and v["argv"]
            and all(isinstance(a, str) for a in v["argv"]))


def load_config():
    cfg = {"dark": True, "show_hidden": False, "pins": [], "unpinned": [],
           "assoc": {}, "keys": {}}
    data = None
    try:
        # #20: utf-8-sig tolerates a BOM (e.g. from Notepad)
        with open(CONFIG_FILE, encoding="utf-8-sig") as f:
            data = json.load(f)
    except Exception:
        try:
            with open(OLD_PIN_FILE, encoding="utf-8-sig") as f:
                old = json.load(f)
            if isinstance(old, list):
                data = {"pins": old}
                # #21: remove the stale migration file
                try:
                    os.unlink(OLD_PIN_FILE)
                except OSError:
                    pass
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
    # #5: atomic write so a crash cannot truncate the live config.
    d = os.path.dirname(CONFIG_FILE) or "."
    try:
        fd, tmp = tempfile.mkstemp(dir=d, prefix=".keyfm.", suffix=".tmp")
    except OSError:
        return False
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(cfg, f, indent=2)
        os.replace(tmp, CONFIG_FILE)
    except Exception:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        return False
    # #22: config contains associations + key bindings — keep it private
    try:
        os.chmod(CONFIG_FILE, 0o600)
    except OSError:
        pass
    return True