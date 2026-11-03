"""Config load / save."""
import json

from .constants import CONFIG_FILE, OLD_PIN_FILE
from .keys import ACTION_INFO, norm_spec


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
        try:
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