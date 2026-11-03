"""Shortcut definitions and key parsing."""
import re

from .constants import ALT_MASK

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
ENTRY_PASS = {"hidden"}

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
    mods, key = split_key(spec)
    return ("Ctrl" in mods or "Alt" in mods
            or re.fullmatch(r"F\d{1,2}", key) is not None)


def key_sequences(spec):
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