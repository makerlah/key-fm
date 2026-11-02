"""Small data models."""


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
        self.extra = extra
        self.orig = orig
        self.info = info


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