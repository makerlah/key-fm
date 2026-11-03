"""keyfm UI - PySide6 port.

Step 7 (with follow-up Explorer-style context menu work):
    - Shift + Right-Click → open a terminal at the current folder.
    - Expanded file-list right-click menu (Cut / Copy / Paste / Create
      shortcut / Delete / Rename / Properties / Open PowerShell here /
      Open with… / Show Windows Context Menu).
    - Native Windows shell context menu via ctypes IContextMenu (so 7-Zip,
      VLC, Git extensions, etc. appear in their real form).

Also the QApplication-vs-Tk shim: ``__main__.py`` (frozen) does

    root = tk.Tk()
    FileManager(root, start_dir)
    root.mainloop()

so we accept that Tk root, withdraw it, schedule its destruction for the
next time Tk's own mainloop is pumped, then block the constructor on
``QApplication.exec()``. When the Qt window closes ``exec`` returns, the
constructor returns, and the pending ``root.destroy`` fires on first
mainloop tick so Tk exits cleanly. Nothing under ``ui.py`` imports tkinter.

The frozen backend modules (``apps.py``, ``config.py``, ``constants.py``,
``drives.py``, ``helpers.py``, ``keys.py``, ``models.py``) are treated as a
read-only third-party library. If a bug is spotted it will be noted here,
never patched in place.

Iteration status
----------------
STEP 1 (done):
    window, QTableView over ``self.shown``, FileListModel, navigate,
    reload, sort (header click), filter (150 ms debounce), path entry,
    status / flash, apply_theme (initial pass).
STEP 2 (done): sidebar, QUICK ACCESS pins, THIS PC, drive usage threads,
               sidebar context menu, Delete-to-unpin, ★ button.
STEP 3 (done): path/filter shortcuts, typeahead, lasso (rubber-band
               selection) with row-fraction edge auto-scroll.
STEP 3b (done): sidebar hover fix + threaded system-wide search in
               THIS PC (QThread, os.walk, streamed results).
STEP 4 (done): tabs (Tab strip, save/load/restore, Ctrl+T, Ctrl+W,
               Ctrl+Tab family, Alt+1..9).
STEP 5 (done): menus, ACTION_DEFS-driven QShortcut bindings,
               shortcuts dialog (show_shortcuts), capture dialog
               (capture_key), conflict reassignment, reset_all.
STEP 6 (done): "+" button on the tab strip; list context menu;
               copy / cut / paste / rename / delete / delete_perm /
               new file / new folder / properties / restore /
               empty bin implemented as real operations + PySide6 dialogs.
STEP 7 (done): Open-with dialog, cfg["assoc"] persistence, Windows
               chooser, async app loading in a QThread, Browse via
               QFileDialog.
STEP 7b (this file): Explorer-style context menu on the file list and
               sidebar; Shift+Right-Click → terminal; "Create shortcut";
               "Show Windows Context Menu" via ctypes IContextMenu.
STEP 8 (done): apply_theme polish for LIGHT & DARK — emoji decorations
               removed, rounded borders, refined header / menu styling,
               padded inputs, modern section headers in the sidebar.
"""
import os
import re
import shutil
import stat
import subprocess
import sys
import time
import traceback

from PySide6.QtCore import (
    Qt, QAbstractTableModel, QModelIndex, QObject, QPoint, QRect, QRunnable,
    QThread, QThreadPool, QTimer, QItemSelection, QItemSelectionModel,
    Signal, QFileInfo,
)
from PySide6.QtGui import (
    QAction, QColor, QCursor, QPalette, QFont, QKeySequence, QShortcut,
)
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QTableView, QLineEdit, QLabel, QAbstractItemView, QHeaderView,
    QToolButton, QTreeWidget, QTreeWidgetItem, QSplitter, QMenu,
    QRubberBand, QTabBar, QDialog, QPushButton, QMessageBox,
    QInputDialog, QFormLayout, QSizePolicy, QCheckBox, QListWidget,
    QListWidgetItem, QFileDialog, QDialogButtonBox, QFileIconProvider,
)

from .constants import (
    APP, IS_WIN, THIS_PC, RECYCLE, VIRTUAL, COLS,
    TITLES, PC_TITLES, BIN_TITLES,
)
from .theme import LIGHT, DARK
from .models import Item, Tab
from .config import load_config, save_config
from .drives import scan_drives, known_folders
from .trash import (
    scan_trash, move_to_trash, win_empty_recycle_bin,
)
from .keys import (
    ACTION_DEFS, ACTION_INFO, ENTRY_PASS, FIXED_KEYS, RESERVED,
    is_safe_key, norm_spec, split_key,
)
from .helpers import (
    human_size, fmt_time, same_loc, exists_loc,
    unique_path, remove_path,
    # NOTE: helpers.brk and helpers.set_clipboard are Tk-coupled and MUST
    # NOT be imported (per task instructions). Clipboard access goes
    # through QApplication.clipboard() instead.
)
from .apps import default_open, launch_with, list_apps


# Default widths (px) used by the file list.
COL_WIDTHS = {"name": 460, "where": 260, "size": 100, "modified": 160}

# Column-name -> alignment. Only "size" is right-aligned.
COL_ALIGN = {"size": Qt.AlignRight | Qt.AlignVCenter}

# Search results carry the containing folder in Item.orig, so the "where"
# column is meaningful (and visible) while a system-wide search is running.
SEARCH_TITLES = {"name": "Name", "where": "Folder", "size": "Size",
                 "modified": "Modified"}

# Drive-usage "extra" text from scan_drives is shaped like
#   "12.3 GB used of 100.0 GB (12.3%)"
# so we can pull out the percentage cheaply when it arrives.
_SIDEBAR_PCT_RE = re.compile(r"\(([\d.]+)%\)")

# Friendly names for the properties dialog.
_KIND_BY_EXT = {
    ".txt": "Text file", ".md": "Markdown file", ".py": "Python file",
    ".js": "JavaScript file", ".ts": "TypeScript file",
    ".json": "JSON file", ".xml": "XML file", ".html": "HTML file",
    ".htm": "HTML file", ".css": "CSS file", ".log": "Log file",
    ".ini": "Configuration file", ".cfg": "Configuration file",
    ".yaml": "YAML file", ".yml": "YAML file",
    ".png": "PNG image", ".jpg": "JPEG image", ".jpeg": "JPEG image",
    ".gif": "GIF image", ".bmp": "Bitmap image", ".svg": "SVG image",
    ".webp": "WebP image", ".tif": "TIFF image", ".tiff": "TIFF image",
    ".mp3": "MP3 audio", ".wav": "WAV audio", ".flac": "FLAC audio",
    ".ogg": "OGG audio", ".m4a": "M4A audio",
    ".mp4": "MP4 video", ".mkv": "Matroska video", ".avi": "AVI video",
    ".mov": "QuickTime video", ".webm": "WebM video",
    ".zip": "ZIP archive", ".tar": "TAR archive", ".gz": "GZip archive",
    ".bz2": "BZip2 archive", ".xz": "XZ archive", ".7z": "7-Zip archive",
    ".rar": "RAR archive",
    ".pdf": "PDF document", ".doc": "Word document", ".docx": "Word document",
    ".xls": "Excel spreadsheet", ".xlsx": "Excel spreadsheet",
    ".ppt": "PowerPoint show", ".pptx": "PowerPoint show",
    ".exe": "Windows application", ".com": "DOS application",
    ".bat": "Batch script", ".cmd": "Command script", ".ps1": "PowerShell script",
    ".sh": "Shell script", ".desktop": "Desktop entry",
    ".app": "macOS application", ".dmg": "macOS disk image",
}

# --- Qt <-> keyfm shortcut name translations --------------------------
_TO_QKEY = {
    "PgDn": "PgDown", "PgUp": "PgUp",
    "Enter": "Return", "Return": "Return",
    "Esc": "Esc", "Escape": "Esc",
    "Delete": "Del", "Del": "Del",
    "Backspace": "Backspace", "BackSpace": "Backspace",
    "Menu": "Menu", "Tab": "Tab", "Space": "Space",
    "Up": "Up", "Down": "Down", "Left": "Left", "Right": "Right",
    "Home": "Home", "End": "End", "Insert": "Ins",
}

_FROM_QKEY = {
    "PgUp": "PgUp", "PgDown": "PgDn",
    "Return": "Enter", "Enter": "Enter",
    "Del": "Delete", "Delete": "Delete",
    "Backspace": "Backspace",
    "Esc": "Esc", "Escape": "Esc",
    "Space": "Space", "Tab": "Tab", "Menu": "Menu",
    "Home": "Home", "End": "End",
    "Up": "Up", "Down": "Down", "Left": "Left", "Right": "Right",
    "Ins": "Insert", "Insert": "Insert",
}


def _to_qsequence(spec):
    if not spec:
        return None
    mods, key = split_key(spec)
    parts = [m for m in ("Ctrl", "Alt", "Shift") if m in mods]
    qkey = _TO_QKEY.get(key, key)
    parts.append(qkey)
    try:
        seq = QKeySequence("+".join(parts))
    except Exception:
        return None
    if seq.isEmpty():
        return None
    return seq


def _spec_from_qt(text):
    if not text:
        return ""
    mods = set()
    key = ""
    for p in text.split("+"):
        if p == "Ctrl":
            mods.add("Ctrl")
        elif p == "Alt":
            mods.add("Alt")
        elif p == "Shift":
            mods.add("Shift")
        elif p in ("Meta", "Super", "Win"):
            return ""
        else:
            key = p
    if not key:
        return ""
    if len(key) == 1 and key.isalpha():
        key = key.upper()
    key = _FROM_QKEY.get(key, key)
    ordered = [m for m in ("Ctrl", "Alt", "Shift") if m in mods]
    return "+".join(ordered + [key])


class FileListModel(QAbstractTableModel):
    """Thin table model over ``FileManager.shown``."""

    _icon_provider = QFileIconProvider()
    _icon_cache = {}

    def __init__(self, parent=None):
        super().__init__(parent)
        self.rows = []
        self.columns = tuple(COLS)
        self.titles = dict(TITLES)

    def _icon_for(self, item):
        if item.is_dir:
            if self._is_drive_root(item.path):
                key = "::drive"
            else:
                key = "::folder"
        else:
            key = (os.path.splitext(item.name)[1].lower()) or "::file"
        cached = self._icon_cache.get(key)
        if cached is not None:
            return cached
        if item.is_dir:
            if key == "::drive":
                ic = self._icon_provider.icon(QFileIconProvider.Drive)
            else:
                ic = self._icon_provider.icon(QFileIconProvider.Folder)
        else:
            ic = self._icon_provider.icon(QFileInfo(item.path))
        self._icon_cache[key] = ic
        return ic

    @staticmethod
    def _is_drive_root(path):
        if not path:
            return False
        if IS_WIN:
            return (len(path) == 3 and path[1] == ":"
                    and path[2] in ("\\", "/"))
        return path == "/"

    def set_rows(self, rows):
        if len(rows) == len(self.rows) and all(
                a is b for a, b in zip(rows, self.rows)):
            return
        self.beginResetModel()
        self.rows = list(rows)
        self.endResetModel()

    def set_layout(self, columns, titles):
        columns = tuple(columns)
        if columns == self.columns and titles == self.titles:
            return
        self._icon_cache.clear()
        self.beginResetModel()
        self.columns = columns
        self.titles = dict(titles)
        self.endResetModel()

    def rowCount(self, parent=QModelIndex()):
        if parent.isValid():
            return 0
        return len(self.rows)

    def columnCount(self, parent=QModelIndex()):
        if parent.isValid():
            return 0
        return len(self.columns)

    def headerData(self, section, orientation, role=Qt.DisplayRole):
        if orientation != Qt.Horizontal:
            return None
        if not 0 <= section < len(self.columns):
            return None
        if role == Qt.DisplayRole:
            title = self.titles.get(self.columns[section])
            return "" if title is None else title
        return None

    def data(self, index, role=Qt.DisplayRole):
        if not index.isValid():
            return None
        row = self.rows[index.row()]
        col = self.columns[index.column()]
        if role == Qt.DisplayRole:
            if col == "name":
                return row.name
            if col == "where":
                return row.orig or ""
            if col == "size":
                if row.is_dir:
                    return ""
                if row.extra:
                    return row.extra
                return human_size(row.size)
            if col == "modified":
                return fmt_time(row.mtime) if row.mtime else ""
        elif role == Qt.DecorationRole:
            if self.columns[index.column()] == "name":
                return self._icon_for(row)
            return None
        elif role == Qt.TextAlignmentRole:
            if col == "size":
                return int(Qt.AlignRight | Qt.AlignVCenter)
            return int(Qt.AlignLeft | Qt.AlignVCenter)
        return None


class _PathEdit(QLineEdit):
    escapePressed = Signal()

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape:
            self.escapePressed.emit()
            return
        super().keyPressEvent(event)


class _FilterEdit(QLineEdit):
    escapePressed = Signal()
    downPressed = Signal()

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape:
            self.escapePressed.emit()
            return
        if event.key() == Qt.Key_Down:
            self.downPressed.emit()
            return
        super().keyPressEvent(event)


class _KeyCaptureDialog(QDialog):
    _MOD_KEYS = (Qt.Key_Control, Qt.Key_Shift, Qt.Key_Alt, Qt.Key_Meta,
                 Qt.Key_AltGr, Qt.Key_unknown)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.spec = None
        self.setWindowTitle("Press a key")
        self.setModal(True)
        self.resize(460, 170)
        v = QVBoxLayout(self)
        v.setContentsMargins(16, 16, 16, 12)
        v.setSpacing(8)
        lbl = QLabel("<b>Press a key combination</b><br>"
                     "Esc cancels. Ctrl/Alt/F-keys are safe; "
                     "other single keys are rejected.")
        lbl.setWordWrap(True)
        v.addWidget(lbl)
        self._hint = QLabel("")
        self._hint.setMinimumHeight(20)
        self._hint.setObjectName("CaptureHint")
        v.addWidget(self._hint)
        row = QHBoxLayout()
        row.addStretch(1)
        cancel = QPushButton("Cancel")
        cancel.clicked.connect(self.reject)
        row.addWidget(cancel)
        v.addLayout(row)
        self.setFocusPolicy(Qt.StrongFocus)

    def showEvent(self, event):
        super().showEvent(event)
        self.setFocus(Qt.OtherFocusReason)

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape:
            self.reject()
            return
        if event.key() in (Qt.Key_Tab, Qt.Key_Backtab):
            mods = []
            if event.modifiers() & Qt.ControlModifier:
                mods.append("Ctrl")
            if event.modifiers() & Qt.AltModifier:
                mods.append("Alt")
            if (event.modifiers() & Qt.ShiftModifier
                    or event.key() == Qt.Key_Backtab):
                mods.append("Shift")
            mods.append("Tab")
            self.spec = "+".join(mods)
            self._hint.setText(f"<i>{self.spec}</i>")
            self.accept()
            return
        if event.key() in self._MOD_KEYS:
            return
        text = ""
        try:
            kc = event.keyCombination()
            text = QKeySequence(kc).toString(QKeySequence.PortableText)
        except Exception:
            text = ""
        if not text:
            return
        spec = _spec_from_qt(text)
        if not spec:
            return
        self.spec = spec
        self._hint.setText(f"<i>{spec}</i>")
        self.accept()


class TabStrip(QTabBar):
    """The horizontal tab strip above the toolbar."""

    newTabRequested = Signal()
    switched = Signal(int)
    closeRequested = Signal(int)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("TabStrip")
        self.setExpanding(False)
        self.setMovable(False)
        self.setTabsClosable(True)
        self.setDocumentMode(True)
        self.setDrawBase(False)
        self.setElideMode(Qt.ElideRight)
        self.setUsesScrollButtons(True)
        self.setFocusPolicy(Qt.NoFocus)
        self.setSizePolicy(QSizePolicy.Preferred, QSizePolicy.Fixed)
        self.currentChanged.connect(self.switched)
        self.tabCloseRequested.connect(self.closeRequested)

    def mouseDoubleClickEvent(self, event):
        if event.button() == Qt.LeftButton:
            pt = event.position().toPoint()
            if self.tabAt(pt) == -1:
                self.newTabRequested.emit()
                event.accept()
                return
        super().mouseDoubleClickEvent(event)


# ================================================= open-with (step 7)
_APP_LOADERS = set()


class _AppLoaderThread(QThread):
    """Discover installed apps in the background."""

    done = Signal(list)
    failed = Signal(str)

    def run(self):
        try:
            apps = list_apps()
        except Exception:
            traceback.print_exc()
            try:
                self.failed.emit("Could not list installed applications")
            except RuntimeError:
                pass
            return
        try:
            self.done.emit(apps)
        except RuntimeError:
            pass


class OpenWithDialog(QDialog):
    """Open-with chooser mirroring the Tkinter dialog."""

    def __init__(self, path, name, ext, saved_assoc, parent=None):
        super().__init__(parent)
        self.path = path
        self.name = name
        self.ext = ext
        self.saved_assoc = dict(saved_assoc) if saved_assoc else None
        self.chosen = None
        self.always = False
        self.forget = False
        self._apps = []
        self._thread = None
        self.setWindowTitle(f"Open “{name}” with…")
        self.setModal(True)
        self.resize(560, 560)
        self._build()

    def _build(self):
        v = QVBoxLayout(self)
        v.setContentsMargins(14, 14, 14, 12)
        v.setSpacing(8)

        title = QLabel(f"Open <b>{self.name}</b> with:")
        title.setWordWrap(True)
        v.addWidget(title)

        self.filter = QLineEdit()
        self.filter.setPlaceholderText("Filter applications…")
        self.filter.setClearButtonEnabled(True)
        self.filter.textChanged.connect(lambda *_: self._populate())
        v.addWidget(self.filter)

        self.list = QListWidget()
        self.list.itemDoubleClicked.connect(
            lambda *_: self._do_open())
        v.addWidget(self.list, 1)

        self.always_cb = QCheckBox(
            f"Always use this app for {self.ext or 'files without extension'}")
        if not self.ext:
            self.always_cb.setEnabled(False)
            self.always_cb.setToolTip("Files without an extension cannot be "
                                      "remembered.")
        elif isinstance(self.saved_assoc, dict) and self.saved_assoc:
            self.always_cb.setChecked(True)
        v.addWidget(self.always_cb)

        row1 = QHBoxLayout()
        self.browse_btn = QPushButton("Browse…")
        self.browse_btn.clicked.connect(self._do_browse)
        row1.addWidget(self.browse_btn)
        self.forget_btn = QPushButton("Forget saved app")
        self.forget_btn.clicked.connect(self._do_forget)
        self.forget_btn.setEnabled(bool(self.saved_assoc))
        row1.addWidget(self.forget_btn)
        row1.addStretch(1)
        v.addLayout(row1)

        if IS_WIN:
            row2 = QHBoxLayout()
            self.win_btn = QPushButton("Windows chooser…")
            self.win_btn.setToolTip(
                "Open the native Windows “Open with” dialog for this file. "
                "Windows will remember your pick itself.")
            self.win_btn.clicked.connect(self._do_windows_chooser)
            row2.addWidget(self.win_btn)
            row2.addStretch(1)
            v.addLayout(row2)

        btns = QDialogButtonBox(
            QDialogButtonBox.Open | QDialogButtonBox.Cancel, self)
        btns.accepted.connect(self._do_open)
        btns.rejected.connect(self.reject)
        v.addWidget(btns)

        self._start_loading()

    def _start_loading(self):
        self.list.clear()
        placeholder = QListWidgetItem("Loading applications…")
        placeholder.setFlags(Qt.NoItemFlags)
        self.list.addItem(placeholder)
        self.list.setEnabled(False)

        th = _AppLoaderThread()
        _APP_LOADERS.add(th)
        th.done.connect(self._on_apps_loaded)
        th.failed.connect(self._on_apps_failed)

        def _cleanup(t=th):
            _APP_LOADERS.discard(t)

        th.finished.connect(_cleanup)
        self._thread = th
        th.start()

    def _on_apps_loaded(self, apps):
        self._apps = list(apps) if apps else []
        self.list.setEnabled(True)
        self._populate()

    def _on_apps_failed(self, msg):
        self.list.setEnabled(True)
        self.list.clear()
        placeholder = QListWidgetItem(msg or "Could not list applications.")
        placeholder.setFlags(Qt.NoItemFlags)
        self.list.addItem(placeholder)

    def _populate(self):
        flt = self.filter.text().strip().lower()
        self.list.blockSignals(True)
        try:
            self.list.clear()
            for app in self._apps:
                name = app.get("name") or ""
                if flt and flt not in name.lower():
                    continue
                item = QListWidgetItem(name)
                item.setData(Qt.UserRole, app)
                self.list.addItem(item)
            target = None
            if self.saved_assoc:
                for i in range(self.list.count()):
                    it = self.list.item(i)
                    data = it.data(Qt.UserRole)
                    if isinstance(data, dict) and self._same_app(
                            data, self.saved_assoc):
                        target = it
                        break
            if target is None and self.list.count() > 0:
                target = self.list.item(0)
            if target is not None:
                self.list.setCurrentItem(target)
                target.setSelected(True)
                self.list.scrollToItem(target)
        finally:
            self.list.blockSignals(False)

    @staticmethod
    def _same_app(a, b):
        if not isinstance(a, dict) or not isinstance(b, dict):
            return False
        a_argv = a.get("argv") or []
        b_argv = b.get("argv") or []
        if not a_argv or not b_argv:
            return False
        return os.path.normcase(str(a_argv[0])) == \
            os.path.normcase(str(b_argv[0]))

    def _do_open(self):
        item = self.list.currentItem()
        if item is None:
            QMessageBox.information(
                self, "Open with",
                "Please pick an application first.")
            return
        app = item.data(Qt.UserRole)
        if not isinstance(app, dict) or not app.get("argv"):
            return
        self.chosen = dict(app)
        self.always = (self.always_cb.isChecked()
                       and self.always_cb.isEnabled()
                       and bool(self.ext))
        self.accept()

    def _do_browse(self):
        start_dir = ""
        if self.saved_assoc:
            first = (self.saved_assoc.get("argv") or [""])[0]
            if first and os.path.isfile(first):
                start_dir = os.path.dirname(first)
        if IS_WIN:
            filters = "Applications (*.exe *.com *.bat *.cmd *.ps1);;All files (*)"
        elif sys.platform == "darwin":
            filters = "Applications (*.app);;All files (*)"
        else:
            filters = "All files (*)"
        fn, _ = QFileDialog.getOpenFileName(
            self, "Choose application", start_dir, filters)
        if not fn:
            return
        if not os.path.isfile(fn):
            return
        if not IS_WIN and sys.platform != "darwin":
            if not os.access(fn, os.X_OK):
                if QMessageBox.question(
                        self, "Not executable",
                        f"{os.path.basename(fn)} does not look executable.\n"
                        f"Use it anyway?",
                        QMessageBox.Yes | QMessageBox.No,
                        QMessageBox.No) != QMessageBox.Yes:
                    return
        name = os.path.basename(fn)
        app = {"name": name, "argv": [fn, "{file}"]}

        already = any(self._same_app(a, app) for a in self._apps)
        if not already:
            self._apps.append(app)
        self.filter.blockSignals(True)
        try:
            self.filter.clear()
        finally:
            self.filter.blockSignals(False)
        self._populate()
        for i in range(self.list.count()):
            item = self.list.item(i)
            data = item.data(Qt.UserRole)
            if isinstance(data, dict) and self._same_app(data, app):
                self.list.setCurrentItem(item)
                item.setSelected(True)
                self.list.scrollToItem(item)
                break

    def _do_forget(self):
        if not self.saved_assoc:
            return
        if QMessageBox.question(
                self, "Forget saved app",
                f"Stop remembering the app for {self.ext or 'files without extension'}?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No) != QMessageBox.Yes:
            return
        self.forget = True
        self.chosen = None
        self.always = False
        self.accept()

    def _do_windows_chooser(self):
        try:
            os.startfile(self.path, "openas")
        except OSError as err:
            QMessageBox.warning(
                self, "Windows chooser",
                f"Could not open the Windows “Open with” dialog:\n{err}")
            return
        self.reject()

    def _shutdown_thread(self):
        th = self._thread
        self._thread = None
        if th is None:
            return
        try:
            th.done.disconnect(self._on_apps_loaded)
        except (RuntimeError, TypeError):
            pass
        try:
            th.failed.disconnect(self._on_apps_failed)
        except (RuntimeError, TypeError):
            pass

    def closeEvent(self, event):
        self._shutdown_thread()
        super().closeEvent(event)

    def accept(self):
        self._shutdown_thread()
        super().accept()

    def reject(self):
        self._shutdown_thread()
        super().reject()


class _SystemSearchThread(QThread):
    """Walk every drive in a background thread, streaming matching items."""

    batch = Signal(list)
    done = Signal()

    BATCH = 48
    MAX_RESULTS = 4000
    SKIP_DIRS = {"$recycle.bin", "system volume information", ".git",
                 "node_modules", "__pycache__", ".svn", ".hg"}

    def __init__(self, query, roots, show_hidden, parent=None):
        super().__init__(parent)
        self.query = query.lower()
        self.roots = list(roots)
        self.show_hidden = show_hidden
        self.truncated = False
        self._stop = False

    def cancel(self):
        self._stop = True

    def _match(self, name):
        if not self.show_hidden and name.startswith("."):
            return False
        return self.query in name.lower()

    def run(self):
        buf = []
        total = 0
        batch_size = self.BATCH
        limit = self.MAX_RESULTS
        try:
            for root in self.roots:
                if self._stop or total >= limit:
                    break
                for dirpath, dirnames, filenames in os.walk(root):
                    if self._stop or total >= limit:
                        break
                    dirnames[:] = [d for d in dirnames
                                   if d.lower() not in self.SKIP_DIRS]
                    for name in dirnames:
                        if self._stop:
                            break
                        if not self._match(name):
                            continue
                        full = os.path.join(dirpath, name)
                        try:
                            mtime = os.stat(full,
                                            follow_symlinks=False).st_mtime
                        except OSError:
                            mtime = 0
                        buf.append(Item(name, full, True, 0, mtime,
                                        name.startswith("."), orig=dirpath))
                        total += 1
                        if len(buf) >= batch_size:
                            self.batch.emit(buf)
                            buf = []
                        if total >= limit:
                            self.truncated = True
                            break
                    if total >= limit:
                        self.truncated = True
                        break
                    for name in filenames:
                        if self._stop:
                            break
                        if not self._match(name):
                            continue
                        full = os.path.join(dirpath, name)
                        try:
                            st = os.stat(full, follow_symlinks=False)
                            size, mtime = st.st_size, st.st_mtime
                        except OSError:
                            size, mtime = 0, 0
                        buf.append(Item(name, full, False, size, mtime,
                                        name.startswith("."), orig=dirpath))
                        total += 1
                        if len(buf) >= batch_size:
                            self.batch.emit(buf)
                            buf = []
                        if total >= limit:
                            self.truncated = True
                            break
        except Exception:
            traceback.print_exc()
        if buf and not self._stop:
            self.batch.emit(buf)
        self.done.emit()


class LassoTableView(QTableView):
    """QTableView with rubber-band lasso selection and keyboard typeahead."""

    DRAG_THRESHOLD = 4
    EDGE_HOT       = 22
    EDGE_MAX_FRAC  = 3.0
    EDGE_TICK_MS   = 28
    TYPE_RESET_MS  = 700

    def __init__(self, parent=None):
        super().__init__(parent)
        self._press_pt = None
        self._press_idx_row = -1
        self._press_mods = Qt.NoModifier
        self._lassoing = False
        self._lasso_mods = Qt.NoModifier
        self._base_selection = []

        self._rubber = QRubberBand(QRubberBand.Rectangle, self.viewport())
        self._rubber.hide()

        self._edge_timer = QTimer(self)
        self._edge_timer.setInterval(self.EDGE_TICK_MS)
        self._edge_timer.timeout.connect(self._edge_tick)
        self._scroll_dir = 0
        self._scroll_frac = 0.0
        self._scroll_accum = 0.0

        self._type_buf = ""
        self._type_timer = QTimer(self)
        self._type_timer.setSingleShot(True)
        self._type_timer.setInterval(self.TYPE_RESET_MS)
        self._type_timer.timeout.connect(self._reset_type_buf)

    def _row_height_px(self):
        vh = self.verticalHeader()
        m = self.model()
        if m is not None and m.rowCount() > 0:
            h = vh.sectionSize(0)
            if h > 0:
                return h
        d = vh.defaultSectionSize()
        return d if d > 0 else 24

    def _selected_rows(self):
        sel = self.selectionModel()
        if sel is None:
            return []
        return [i.row() for i in sel.selectedRows(0)]

    def mousePressEvent(self, event):
        if event.button() == Qt.RightButton:
            self._finish_lasso()
            super().mousePressEvent(event)
            return
        if event.button() != Qt.LeftButton:
            super().mousePressEvent(event)
            return
        pt = event.position().toPoint()
        idx = self.indexAt(pt)
        if idx.isValid():
            self._press_pt = None
            self._press_idx_row = -1
            super().mousePressEvent(event)
            return
        self.setFocus(Qt.MouseFocusReason)
        self._press_pt = pt
        self._press_idx_row = -1
        self._press_mods = event.modifiers()
        if not (self._press_mods & (Qt.ControlModifier | Qt.ShiftModifier)):
            self.clearSelection()
        self._lassoing = False
        event.accept()

    def mouseMoveEvent(self, event):
        if self._press_pt is None:
            super().mouseMoveEvent(event)
            return
        pt = event.position().toPoint()
        if not self._lassoing:
            if (pt - self._press_pt).manhattanLength() < self.DRAG_THRESHOLD:
                event.accept()
                return
            self._lassoing = True
            self._lasso_mods = self._press_mods
            self._base_selection = list(self._selected_rows())
        rect = self._lasso_rect(self._press_pt, pt)
        self._rubber.setGeometry(rect)
        if not self._rubber.isVisible():
            self._rubber.show()
        self._apply_lasso(rect)
        self._update_edge_scroll(pt)
        event.accept()

    def mouseReleaseEvent(self, event):
        if event.button() != Qt.LeftButton:
            super().mouseReleaseEvent(event)
            return
        if self._lassoing:
            self._finish_lasso()
            event.accept()
            return
        if self._press_pt is not None:
            self._press_pt = None
            event.accept()
            return
        super().mouseReleaseEvent(event)

    def _lasso_rect(self, a, b):
        r = QRect(a, b).normalized()
        return r.intersected(self.viewport().rect())

    def _apply_lasso(self, rect):
        model = self.model()
        sel = self.selectionModel()
        if model is None or sel is None:
            return
        w = self.viewport().width()
        hit = set()
        for r in range(model.rowCount()):
            top = self.rowViewportPosition(r)
            h = self.rowHeight(r)
            if h <= 0:
                continue
            if QRect(0, top, w, h).intersects(rect):
                hit.add(r)
        if self._lasso_mods & Qt.ControlModifier:
            rows = set(self._base_selection) | hit
        else:
            rows = hit
        if not rows:
            sel.clearSelection()
            return
        qsel = QItemSelection()
        last_col = model.columnCount() - 1
        for r in sorted(rows):
            qsel.select(model.index(r, 0), model.index(r, last_col))
        sel.select(qsel, QItemSelectionModel.ClearAndSelect)

    def _finish_lasso(self):
        self._lassoing = False
        self._rubber.hide()
        self._edge_timer.stop()
        self._scroll_dir = 0
        self._scroll_frac = 0.0
        self._scroll_accum = 0.0
        self._press_pt = None
        self._base_selection = []

    def _update_edge_scroll(self, pt):
        y = pt.y()
        h = self.viewport().height()
        zone = self.EDGE_HOT
        new_dir = 0
        frac = 0.0
        if y < zone:
            overshoot = zone - y
            frac = overshoot / max(self._row_height_px(), 1)
            if frac > self.EDGE_MAX_FRAC:
                frac = self.EDGE_MAX_FRAC
            new_dir = -1
        elif y > h - zone:
            overshoot = y - (h - zone)
            frac = overshoot / max(self._row_height_px(), 1)
            if frac > self.EDGE_MAX_FRAC:
                frac = self.EDGE_MAX_FRAC
            new_dir = 1
        if new_dir == 0:
            self._edge_timer.stop()
            self._scroll_dir = 0
            self._scroll_frac = 0.0
            self._scroll_accum = 0.0
            return
        if new_dir != self._scroll_dir:
            self._scroll_accum = 0.0
        self._scroll_dir = new_dir
        self._scroll_frac = frac
        if not self._edge_timer.isActive():
            self._edge_timer.start()

    def _edge_tick(self):
        if not self._lassoing or self._scroll_dir == 0 or self._scroll_frac <= 0.0:
            self._edge_timer.stop()
            return
        self._scroll_accum += self._scroll_frac
        sb = self.verticalScrollBar()
        moved = 0
        while self._scroll_accum >= 1.0:
            self._scroll_accum -= 1.0
            if self._scroll_dir < 0:
                if sb.value() <= sb.minimum():
                    self._scroll_accum = 0.0
                    self._edge_timer.stop()
                    break
                sb.setValue(sb.value() - 1)
            else:
                if sb.value() >= sb.maximum():
                    self._scroll_accum = 0.0
                    self._edge_timer.stop()
                    break
                sb.setValue(sb.value() + 1)
            moved += 1
        if moved:
            self._apply_lasso(self._rubber.geometry())

    def _reset_type_buf(self):
        self._type_buf = ""

    def _handle_typeahead(self, event):
        if event.modifiers() & (Qt.ControlModifier | Qt.AltModifier
                                | Qt.MetaModifier):
            return False
        text = event.text()
        if not text or not text.isprintable() or text == " ":
            return False
        model = self.model()
        if model is None or not hasattr(model, "rows") or not model.rows:
            return False
        self._type_buf = self._type_buf + text.lower()
        self._type_timer.start()
        buf = self._type_buf
        same = len(buf) >= 2 and all(c == buf[0] for c in buf)
        query = buf[0] if same else buf
        n = len(model.rows)
        cur = self.currentIndex().row()
        if cur < 0:
            cur = 0
        start = (cur + 1) if same else cur
        for i in range(n):
            r = (start + i) % n
            if model.rows[r].name.lower().startswith(query):
                idx = model.index(r, 0)
                self.selectionModel().setCurrentIndex(
                    idx,
                    QItemSelectionModel.ClearAndSelect
                    | QItemSelectionModel.Rows)
                self.scrollTo(idx, QAbstractItemView.EnsureVisible)
                return True
        return True

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape and self._lassoing:
            self._finish_lasso()
            event.accept()
            return
        if self._handle_typeahead(event):
            event.accept()
            return
        super().keyPressEvent(event)


class _SidebarUsageSignals(QObject):
    ready = Signal(str, str)


class _DriveUsageTask(QRunnable):
    def __init__(self, path, signals):
        super().__init__()
        self.path = path
        self.signals = signals

    def run(self):
        try:
            total, _used, free = shutil.disk_usage(self.path)
        except OSError:
            return
        used = total - free
        percent = (used / total * 100) if total else 0
        extra = (f"{human_size(used)} used of {human_size(total)} "
                 f"({percent:.1f}%)")
        try:
            self.signals.ready.emit(self.path, extra)
        except RuntimeError:
            pass


class Sidebar(QTreeWidget):
    """Left-hand nav: QUICK ACCESS (pins + known folders) and THIS PC."""

    navigateTo = Signal(str)
    escapePressed = Signal()
    unpinFromSidebar = Signal(str)
    contextMenuRequested = Signal(str, object)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setObjectName("Sidebar")
        self.setHeaderHidden(True)
        self.setIndentation(10)
        self.setRootIsDecorated(False)
        self.setExpandsOnDoubleClick(False)
        self.setSelectionMode(QAbstractItemView.SingleSelection)
        self.setUniformRowHeights(True)
        self.setAnimated(False)
        self.setFocusPolicy(Qt.StrongFocus)
        self.setColumnCount(1)
        self.setMouseTracking(True)
        self.viewport().setMouseTracking(True)
        self.viewport().setContextMenuPolicy(Qt.CustomContextMenu)
        self.viewport().customContextMenuRequested.connect(
            self._on_context_menu)
        self.itemClicked.connect(self._on_item_clicked)

    def _on_item_clicked(self, item, _col):
        if item is None:
            return
        path = item.data(0, Qt.UserRole)
        if path is not None:
            self.navigateTo.emit(path)

    def _on_context_menu(self, pos):
        item = self.itemAt(pos)
        if item is None:
            return
        path = item.data(0, Qt.UserRole)
        if path is None:
            return
        self.contextMenuRequested.emit(path, self.viewport().mapToGlobal(pos))

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape:
            self.escapePressed.emit()
            return
        if event.key() == Qt.Key_Delete:
            item = self.currentItem()
            if item is not None:
                path = item.data(0, Qt.UserRole)
                if path is not None:
                    self.unpinFromSidebar.emit(path)
                    return
        if event.key() in (Qt.Key_Return, Qt.Key_Enter):
            item = self.currentItem()
            if item is not None:
                path = item.data(0, Qt.UserRole)
                if path is not None:
                    self.navigateTo.emit(path)
                    return
        super().keyPressEvent(event)

    def make_section(self, text, dim_color, clickable=False, path=None):
        item = QTreeWidgetItem([text.upper()])
        if clickable:
            item.setFlags(Qt.ItemIsEnabled | Qt.ItemIsSelectable)
        else:
            item.setFlags(Qt.ItemIsEnabled)
        f = item.font(0)
        f.setBold(True)
        f.setPointSize(max(f.pointSize() - 1, 7))
        f.setLetterSpacing(QFont.AbsoluteSpacing, 1.0)
        item.setFont(0, f)
        item.setForeground(0, QColor(dim_color))
        if path is not None:
            item.setData(0, Qt.UserRole, path)
        self.addTopLevelItem(item)
        return item

    def add_nav_item(self, parent, text, path, tooltip=""):
        item = QTreeWidgetItem([text])
        item.setFlags(Qt.ItemIsEnabled | Qt.ItemIsSelectable)
        item.setData(0, Qt.UserRole, path)
        item.setToolTip(0, tooltip or str(path))
        parent.addChild(item)
        return item

    def apply_header_colors(self, dim_color):
        for i in range(self.topLevelItemCount()):
            top = self.topLevelItem(i)
            top.setForeground(0, QColor(dim_color))

    def sync_selection(self, path):
        for i in range(self.topLevelItemCount()):
            top = self.topLevelItem(i)
            tpath = top.data(0, Qt.UserRole)
            if tpath is not None and same_loc(tpath, path):
                self.setCurrentItem(top)
                self.scrollToItem(top)
                return
            for j in range(top.childCount()):
                child = top.child(j)
                cpath = child.data(0, Qt.UserRole)
                if cpath is not None and same_loc(cpath, path):
                    self.setCurrentItem(child)
                    self.scrollToItem(child)
                    return
        self.clearSelection()
        self.setCurrentItem(None)


class FileManager(QMainWindow):
    def __init__(self, root, start):
        try:
            if root is not None and hasattr(root, "after"):
                root.withdraw()
                root.after(0, root.destroy)
        except Exception:
            traceback.print_exc()

        app = QApplication.instance()
        if app is None:
            app = QApplication([])
        self.app = app

        super().__init__()

        # ---- config / state ------------------------------------------
        self.cfg = load_config()
        self.dark = self.cfg["dark"]
        self.show_hidden = self.cfg["show_hidden"]
        self.palette = DARK if self.dark else LIGHT

        self.cwd = None
        self.entries = []
        self.shown = []
        self.back = []
        self.fwd = []
        self.sort_col = "name"
        self.sort_reverse = False
        self.clip = []
        self.clip_mode = "copy"
        self._suppress_filter = False

        self._drive_usage = {}
        self._usage_signals = None

        self._search_active = False
        self._search_thread = None
        self._search_query = ""
        self._search_results = []
        self._search_truncated = False
        self._retired = []

        self.tabs = [Tab()]
        self.cur = 0
        self._switching_tab = False
        self.tab_bar = None

        self._menu_actions = {}
        self._shortcut_list = []

        self._flash_timer = QTimer(self)
        self._flash_timer.setSingleShot(True)
        self._flash_timer.timeout.connect(self.update_status)

        self._filter_timer = QTimer(self)
        self._filter_timer.setSingleShot(True)
        self._filter_timer.setInterval(150)
        self._filter_timer.timeout.connect(self._apply_filter_debounced)

        # ---- window --------------------------------------------------
        self.setWindowTitle(APP)
        self.resize(1180, 700)
        self.setMinimumSize(760, 420)

        self._build_ui()
        self.apply_theme()

        self._usage_signals = _SidebarUsageSignals(self)
        self._usage_signals.ready.connect(self._on_usage_ready)
        self._populate_sidebar()

        home = os.path.expanduser("~")
        if not self.navigate(start, record=False):
            self.navigate(home, record=False)

        self.show()
        self.app.exec()

    # ------------------------------------------------------------- UI build
    def _flat_btn(self, text, cmd):
        b = QToolButton()
        b.setText(text)
        b.setAutoRaise(True)
        b.setCursor(Qt.PointingHandCursor)
        b.clicked.connect(lambda *_: cmd())
        f = b.font()
        f.setPointSize(max(f.pointSize(), 12))
        b.setFont(f)
        return b

    def _build_ui(self):
        central = QWidget(self)
        self.setCentralWidget(central)
        v = QVBoxLayout(central)
        v.setContentsMargins(8, 6, 8, 6)
        v.setSpacing(6)

        # --- tab strip + "+" button ------------------------------------
        tab_row = QHBoxLayout()
        tab_row.setContentsMargins(0, 0, 0, 0)
        tab_row.setSpacing(2)
        self.tab_bar = TabStrip()
        self.tab_bar.newTabRequested.connect(self._on_new_tab_requested)
        self.tab_bar.switched.connect(self._on_tab_switched)
        self.tab_bar.closeRequested.connect(self._on_tab_close_requested)
        tab_row.addWidget(self.tab_bar, 0)

        self.btn_new_tab = QToolButton()
        self.btn_new_tab.setObjectName("NewTabButton")
        self.btn_new_tab.setText("+")
        self.btn_new_tab.setToolTip("New tab (Ctrl+T)")
        self.btn_new_tab.setCursor(Qt.PointingHandCursor)
        self.btn_new_tab.setAutoRaise(True)
        self.btn_new_tab.clicked.connect(lambda *_: self.new_tab())
        tab_row.addWidget(self.btn_new_tab, 0)
        tab_row.addStretch(1)
        v.addLayout(tab_row)

        # --- toolbar row -----------------------------------------------
        top = QHBoxLayout()
        top.setSpacing(2)
        self.btn_back    = self._flat_btn("←", self.go_back)
        self.btn_fwd     = self._flat_btn("→", self.go_forward)
        self.btn_up      = self._flat_btn("↑", self.go_parent)
        self.btn_refresh = self._flat_btn("↻", self.reload)
        for b in (self.btn_back, self.btn_fwd, self.btn_up, self.btn_refresh):
            top.addWidget(b)

        self.path_edit = _PathEdit()
        self.path_edit.setClearButtonEnabled(False)
        self.path_edit.returnPressed.connect(self.on_path_enter)
        self.path_edit.escapePressed.connect(self.on_path_escape)
        top.addWidget(self.path_edit, 1)

        self.btn_pin = self._flat_btn("Pin", self.toggle_pin_current)
        top.addWidget(self.btn_pin)

        self.filter_edit = _FilterEdit()
        self.filter_edit.setMaximumWidth(220)
        self.filter_edit.textChanged.connect(self.on_filter_change)
        self.filter_edit.escapePressed.connect(self.on_filter_escape)
        self.filter_edit.downPressed.connect(self.focus_tree)
        top.addWidget(self.filter_edit)

        v.addLayout(top)

        # --- sidebar + file list ---------------------------------------
        self.sidebar = Sidebar()
        self.sidebar.setMinimumWidth(160)
        self.sidebar.setMaximumWidth(520)
        self.sidebar.navigateTo.connect(self._on_sidebar_navigate)
        self.sidebar.escapePressed.connect(self.focus_tree)
        self.sidebar.unpinFromSidebar.connect(self._on_sidebar_unpin)
        self.sidebar.contextMenuRequested.connect(self._on_sidebar_context)

        self.model = FileListModel(self)
        self.view = LassoTableView()
        self.view.setModel(self.model)
        self.view.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.view.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.view.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.view.setShowGrid(False)
        self.view.setSortingEnabled(False)
        self.view.verticalHeader().setVisible(False)
        self.view.verticalHeader().setDefaultSectionSize(28)
        self.view.setWordWrap(False)

        # Right-click context menu for the file list.
        self.view.setContextMenuPolicy(Qt.CustomContextMenu)
        self.view.customContextMenuRequested.connect(self._on_list_context)

        h = self.view.horizontalHeader()
        h.setHighlightSections(False)
        h.setSectionsClickable(True)
        h.setSectionResizeMode(QHeaderView.Interactive)
        h.setStretchLastSection(False)
        h.sectionClicked.connect(self._header_clicked)

        self.view.selectionModel().selectionChanged.connect(
            lambda *_: self.update_status())

        self.splitter = QSplitter(Qt.Horizontal)
        self.splitter.addWidget(self.sidebar)
        self.splitter.addWidget(self.view)
        self.splitter.setStretchFactor(0, 0)
        self.splitter.setStretchFactor(1, 1)
        self.splitter.setSizes([220, 960])
        self.splitter.setChildrenCollapsible(False)
        v.addWidget(self.splitter, 1)

        # --- status bar -------------------------------------------------
        self.status = QLabel("")
        self.status.setObjectName("StatusLabel")
        v.addWidget(self.status)

        # --- menus + unified shortcuts -------------------------------
        self._build_menus()
        self._bind_shortcuts()

        self._rebuild_tab_strip()

    # ----------------------------------------------------------- theme
    def apply_theme(self):
        P = DARK if self.dark else LIGHT
        self.palette = P

        app = QApplication.instance()
        if app is not None:
            pal = QPalette()
            pal.setColor(QPalette.Window,          QColor(P["bg"]))
            pal.setColor(QPalette.WindowText,      QColor(P["fg"]))
            pal.setColor(QPalette.Base,            QColor(P["entry"]))
            pal.setColor(QPalette.AlternateBase,   QColor(P["tab"]))
            pal.setColor(QPalette.Text,            QColor(P["fg"]))
            pal.setColor(QPalette.Button,          QColor(P["tab"]))
            pal.setColor(QPalette.ButtonText,      QColor(P["fg"]))
            pal.setColor(QPalette.Highlight,       QColor(P["sel_bg"]))
            pal.setColor(QPalette.HighlightedText, QColor(P["sel_fg"]))
            pal.setColor(QPalette.ToolTipBase,     QColor(P["bg"]))
            pal.setColor(QPalette.ToolTipText,     QColor(P["fg"]))
            app.setPalette(pal)

        self.setStyleSheet(f"""
            QMainWindow, QWidget {{
                background: {P['bg']};
                color: {P['fg']};
            }}
            QToolButton {{
                background: transparent;
                color: {P['fg']};
                border: none;
                border-radius: 5px;
                padding: 4px 10px;
            }}
            QToolButton:hover {{ background: {P['hover']}; }}
            QToolButton:pressed {{ background: {P['sel_bg']}; }}
            QToolButton#NewTabButton {{
                background: transparent;
                color: {P['dim']};
                border: none;
                border-radius: 5px;
                padding: 0 10px;
                font-size: 15pt;
                font-weight: bold;
                min-width: 16px;
            }}
            QToolButton#NewTabButton:hover {{
                background: {P['hover']};
                color: {P['fg']};
            }}
            QTableView {{
                background: {P['field']};
                color: {P['fg']};
                selection-background-color: {P['sel_bg']};
                selection-color: {P['sel_fg']};
                gridline-color: transparent;
                border: 1px solid {P['border']};
                border-radius: 6px;
                alternate-background-color: {P['side']};
                outline: none;
            }}
            QTableView::item {{
                padding: 4px 10px;
                border: none;
            }}
            QHeaderView {{
                background: {P['head']};
            }}
            QHeaderView::section {{
                background: {P['head']};
                color: {P['dim']};
                border: none;
                border-bottom: 1px solid {P['border']};
                padding: 8px 10px;
                font-weight: 600;
            }}
            QHeaderView::section:hover {{
                background: {P['hover']};
                color: {P['fg']};
            }}
            QLineEdit {{
                background: {P['entry']};
                color: {P['fg']};
                border: 1px solid {P['border']};
                border-radius: 6px;
                padding: 6px 10px;
                selection-background-color: {P['sel_bg']};
                selection-color: {P['sel_fg']};
            }}
            QLineEdit:focus {{ border: 1px solid {P['accent']}; }}
            QLabel#StatusLabel {{
                background: {P['side']};
                color: {P['dim']};
                padding: 6px 14px;
                border-radius: 6px;
            }}
            QLabel#CaptureHint {{
                color: {P['accent']};
                font-size: 14pt;
            }}
            QTreeWidget#Sidebar {{
                background: {P['side']};
                color: {P['fg']};
                border: 1px solid {P['border']};
                border-radius: 6px;
                outline: none;
                padding: 4px;
                show-decoration-selected: 1;
            }}
            QTreeWidget#Sidebar::item {{
                background-color: transparent;
                padding: 6px 8px;
                border-radius: 4px;
                margin: 1px 2px;
            }}
            QTreeWidget#Sidebar::item:hover {{
                background-color: {P['hover']};
            }}
            QTreeWidget#Sidebar::item:selected {{
                background-color: {P['sel_bg']};
                color: {P['sel_fg']};
            }}
            QSplitter::handle {{
                background: {P['bg']};
            }}
            QSplitter::handle:hover {{
                background: {P['hover']};
            }}
            QTabBar#TabStrip {{
                background: transparent;
                qproperty-drawBase: 0;
            }}
            QTabBar#TabStrip::tab {{
                background: {P['tab']};
                color: {P['dim']};
                border: 1px solid {P['border']};
                border-bottom: none;
                padding: 6px 14px;
                margin-right: 3px;
                border-top-left-radius: 6px;
                border-top-right-radius: 6px;
                min-width: 90px;
            }}
            QTabBar#TabStrip::tab:selected {{
                background: {P['tab_sel']};
                color: {P['fg']};
            }}
            QTabBar#TabStrip::tab:hover:!selected {{
                background: {P['hover']};
                color: {P['fg']};
            }}
            QTabBar#TabStrip::close-button {{
                background: transparent;
                border: none;
                border-radius: 3px;
            }}
            QTabBar#TabStrip::close-button:hover {{
                background: {P['danger']};
            }}
            QMenuBar {{
                background: {P['bg']};
                color: {P['fg']};
            }}
            QMenuBar::item {{
                background: transparent;
                padding: 4px 10px;
                border-radius: 4px;
            }}
            QMenuBar::item:selected {{ background: {P['hover']}; }}
            QMenu {{
                background: {P['menu_bg']};
                color: {P['fg']};
                border: 1px solid {P['border']};
                border-radius: 6px;
                padding: 4px;
            }}
            QMenu::item {{
                padding: 6px 24px;
                border-radius: 4px;
                background: transparent;
            }}
            QMenu::item:selected {{
                background: {P['menu_active']};
                color: {P['fg']};
            }}
            QMenu::item:disabled {{ color: {P['dim']}; }}
            QMenu::separator {{
                height: 1px;
                background: {P['border']};
                margin: 4px 8px;
            }}
            QListWidget {{
                background: {P['field']};
                color: {P['fg']};
                border: 1px solid {P['border']};
                border-radius: 6px;
                outline: none;
            }}
            QListWidget::item {{
                padding: 5px 8px;
                border-radius: 4px;
            }}
            QListWidget::item:hover {{ background: {P['hover']}; }}
            QListWidget::item:selected {{
                background: {P['sel_bg']};
                color: {P['sel_fg']};
            }}
            QCheckBox {{ color: {P['fg']}; }}
            QPushButton {{
                background: {P['tab']};
                color: {P['fg']};
                border: 1px solid {P['border']};
                border-radius: 6px;
                padding: 5px 14px;
            }}
            QPushButton:hover {{ background: {P['hover']}; }}
            QPushButton:pressed {{ background: {P['sel_bg']}; }}
            QDialog {{ background: {P['bg']}; color: {P['fg']}; }}
        """)

        if getattr(self, "sidebar", None) is not None:
            self.sidebar.apply_header_colors(P["dim"])

        self._update_pin_button()

    def toggle_dark(self):
        self.dark = not self.dark
        self.apply_theme()
        self.cfg["dark"] = self.dark
        save_config(self.cfg)
        self.flash("Dark mode on" if self.dark else "Light mode on")

    def toggle_hidden(self):
        self.show_hidden = not self.show_hidden
        self.cfg["show_hidden"] = self.show_hidden
        save_config(self.cfg)
        self.reload()

    # -------------------------------------------------------- menu bar
    def _build_menus(self):
        mb = self.menuBar()
        try:
            mb.setNativeMenuBar(False)
        except Exception:
            pass

        def add(menu, aid, sep_before=False):
            if sep_before:
                menu.addSeparator()
            info = ACTION_INFO.get(aid)
            if info is None:
                return
            title = info[1]
            specs = self._effective_keys(aid)
            text = f"{title}\t{specs[0]}" if specs else title
            act = QAction(text, self)
            act.triggered.connect(
                lambda _checked=False, aid=aid: self._run_action(aid))
            menu.addAction(act)
            self._menu_actions.setdefault(aid, []).append(act)

        file_menu = mb.addMenu("&File")
        add(file_menu, "new_tab")
        add(file_menu, "close_tab")
        add(file_menu, "open", sep_before=True)
        add(file_menu, "open_with")
        add(file_menu, "properties")
        add(file_menu, "quit", sep_before=True)

        edit_menu = mb.addMenu("&Edit")
        add(edit_menu, "copy")
        add(edit_menu, "cut")
        add(edit_menu, "paste")
        add(edit_menu, "copy_path")
        add(edit_menu, "rename", sep_before=True)
        add(edit_menu, "delete")
        add(edit_menu, "delete_perm")
        add(edit_menu, "new_file", sep_before=True)
        add(edit_menu, "new_folder")
        add(edit_menu, "select_all", sep_before=True)

        view_menu = mb.addMenu("&View")
        add(view_menu, "dark")
        add(view_menu, "hidden")
        add(view_menu, "sort_name", sep_before=True)
        add(view_menu, "sort_size")
        add(view_menu, "sort_date")
        add(view_menu, "refresh", sep_before=True)

        go_menu = mb.addMenu("&Go")
        add(go_menu, "back")
        add(go_menu, "forward")
        add(go_menu, "go_parent")
        add(go_menu, "home", sep_before=True)
        add(go_menu, "this_pc")
        add(go_menu, "recycle")
        add(go_menu, "edit_path", sep_before=True)
        add(go_menu, "filter")
        add(go_menu, "sidebar")

        tools_menu = mb.addMenu("Tool&s")
        add(tools_menu, "shortcuts")
        tools_menu.addSeparator()
        add(tools_menu, "pin")
        add(tools_menu, "unpin")
        tools_menu.addSeparator()
        add(tools_menu, "empty_bin")
        tools_menu.addSeparator()
        reset_act = QAction("Reset all shortcuts", self)
        reset_act.triggered.connect(self._menu_reset_all)
        tools_menu.addAction(reset_act)

        self._menus = {
            "file": file_menu, "edit": edit_menu, "view": view_menu,
            "go": go_menu, "tools": tools_menu,
        }

    def _refresh_menu_shortcuts(self):
        for aid, acts in self._menu_actions.items():
            info = ACTION_INFO.get(aid)
            if info is None:
                continue
            title = info[1]
            specs = self._effective_keys(aid)
            text = f"{title}\t{specs[0]}" if specs else title
            for a in acts:
                a.setText(text)

    def _menu_reset_all(self):
        if QMessageBox.question(
                self, "Reset shortcuts",
                "Restore all keyboard shortcuts to their defaults?",
                QMessageBox.Yes | QMessageBox.No) == QMessageBox.Yes:
            self.reset_all()

    # -------------------------------------------------- keybinding core
    def _effective_keys(self, aid):
        user = self.cfg.get("keys") or {}
        saved = user.get(aid)
        if isinstance(saved, list):
            return [s for s in saved if isinstance(s, str)]
        info = ACTION_INFO.get(aid)
        if info is None:
            return []
        return list(info[3])

    def _bind_shortcuts(self):
        for sc in self._shortcut_list:
            try:
                sc.setEnabled(False)
                sc.deleteLater()
            except RuntimeError:
                pass
        self._shortcut_list = []
        used = set()

        for aid, title, category, defaults, scope in ACTION_DEFS:
            for raw in self._effective_keys(aid):
                spec = norm_spec(raw) if isinstance(raw, str) else ""
                if not spec or spec in used:
                    continue
                seq = _to_qsequence(spec)
                if seq is None:
                    continue
                if (scope == "all" or aid in ENTRY_PASS) and is_safe_key(spec):
                    host, ctx = self, Qt.WindowShortcut
                else:
                    host, ctx = self.view, Qt.WidgetWithChildrenShortcut
                try:
                    sc = QShortcut(seq, host)
                except Exception:
                    continue
                sc.setContext(ctx)
                sc.activated.connect(
                    lambda aid=aid: self._run_action(aid))
                self._shortcut_list.append(sc)
                used.add(spec)

        for n in range(1, 10):
            seq = QKeySequence(f"Alt+{n}")
            sc = QShortcut(seq, self)
            sc.setContext(Qt.WindowShortcut)
            sc.activated.connect(lambda _idx=n - 1: self.switch_tab(_idx))
            self._shortcut_list.append(sc)

        self._refresh_menu_shortcuts()

    def reset_all(self):
        self.cfg["keys"] = {}
        save_config(self.cfg)
        self._bind_shortcuts()
        self.flash("Keyboard shortcuts reset to defaults")

    # ------------------------------------------------ action dispatcher
    def _run_action(self, aid):
        fn = getattr(self, "_act_" + aid, None)
        if fn is None:
            info = ACTION_INFO.get(aid)
            title = info[1] if info else aid
            self.flash(f"“{title}” arrives in a later step")
            return
        try:
            fn()
        except Exception:
            traceback.print_exc()

    # --- tabs
    def _act_new_tab(self): self.new_tab()
    def _act_close_tab(self): self.close_tab()
    def _act_next_tab(self): self.next_tab()
    def _act_prev_tab(self): self.prev_tab()

    # --- navigation
    def _act_open(self): self.open_current()
    def _act_open_folder(self): self.open_current_folder()
    def _act_go_parent(self): self.go_parent()
    def _act_back(self): self.go_back()
    def _act_forward(self): self.go_forward()
    def _act_home(self): self.navigate(os.path.expanduser("~"))
    def _act_this_pc(self): self.navigate(THIS_PC)
    def _act_recycle(self): self.navigate(RECYCLE)
    def _act_edit_path(self): self.focus_path()
    def _act_filter(self): self.focus_filter()
    def _act_sidebar(self): self.toggle_sidebar_focus()
    def _act_first(self): self._focus_row(0)
    def _act_last(self): self._focus_row(len(self.shown) - 1)
    def _act_page_up(self):
        cur = self.view.currentIndex().row()
        if cur < 0: cur = 0
        self._focus_row(cur - 15)
    def _act_page_down(self):
        cur = self.view.currentIndex().row()
        if cur < 0: cur = 0
        self._focus_row(cur + 15)

    # --- files
    def _act_select_all(self): self.view.selectAll()
    def _act_copy_path(self): self.copy_path()
    def _act_copy(self): self.do_copy()
    def _act_cut(self): self.do_cut()
    def _act_paste(self): self.do_paste()
    def _act_rename(self): self.do_rename()
    def _act_delete(self): self.do_delete(permanent=False)
    def _act_delete_perm(self): self.do_delete(permanent=True)
    def _act_new_file(self): self.do_new_file()
    def _act_new_folder(self): self.do_new_folder()
    def _act_properties(self): self.do_properties()
    def _act_context_menu(self): self.do_context_menu()
    def _act_restore(self): self.do_restore()
    def _act_empty_bin(self): self.do_empty_bin()
    def _act_open_with(self): self.do_open_with()

    # --- view
    def _act_sort_name(self): self.set_sort("name")
    def _act_sort_size(self): self.set_sort("size")
    def _act_sort_date(self): self.set_sort("modified")
    def _act_hidden(self): self.toggle_hidden()
    def _act_dark(self): self.toggle_dark()
    def _act_refresh(self): self.reload()

    # --- quick access
    def _act_pin(self): self.toggle_pin_current()
    def _act_unpin(self):
        if self.cwd and self.cwd not in VIRTUAL:
            self._unpin_and_flash(self.cwd)

    # --- program
    def _act_quit(self): self.close()
    def _act_shortcuts(self): self.show_shortcuts()
    def _act_menu_bar(self): self.open_menu_bar()

    # --------------------------------------------------------- utilities
    def _focus_row(self, row):
        if not self.shown:
            return
        row = max(0, min(len(self.shown) - 1, row))
        self.view.setFocus()
        idx = self.model.index(row, 0)
        self.view.selectionModel().setCurrentIndex(
            idx,
            QItemSelectionModel.ClearAndSelect | QItemSelectionModel.Rows)
        self.view.scrollTo(idx, QAbstractItemView.EnsureVisible)

    def _select_by_name(self, name):
        for r, it in enumerate(self.shown):
            if it.name == name:
                self.view.setFocus()
                self.view.selectRow(r)
                idx = self.model.index(r, 0)
                self.view.setCurrentIndex(idx)
                self.view.scrollTo(idx, QAbstractItemView.EnsureVisible)
                return

    def _current_item(self):
        idx = self.view.currentIndex()
        if idx.isValid() and 0 <= idx.row() < len(self.shown):
            return self.shown[idx.row()]
        return None

    def _selected_items(self):
        sm = self.view.selectionModel()
        if sm is None:
            return []
        return [self.shown[i.row()] for i in sm.selectedRows(0)
                if 0 <= i.row() < len(self.shown)]

    def _focus_item_name(self):
        idx = self.view.currentIndex()
        if idx.isValid() and 0 <= idx.row() < len(self.shown):
            return self.shown[idx.row()].name
        return None

    def _kind_label(self, name):
        ext = os.path.splitext(name)[1].lower()
        if not ext:
            return "File"
        return _KIND_BY_EXT.get(ext, f"{ext[1:].upper()} file")

    def _assoc_for(self, name):
        """Return the cfg["assoc"] entry for this filename, or None."""
        ext = os.path.splitext(name)[1].lower()
        return (self.cfg.get("assoc") or {}).get(ext), ext

    def _forget_assoc(self, ext):
        try:
            assoc = self.cfg.get("assoc") or {}
            if ext in assoc:
                del assoc[ext]
                save_config(self.cfg)
        except (KeyError, OSError):
            pass

    # ------------------------------------------------------------- opening
    def open_current(self):
        it = self._current_item()
        if it is None:
            return
        if it.is_dir:
            self.navigate(it.path)
            return

        # 1. User's saved association for this extension wins.
        saved, ext = self._assoc_for(it.name)
        if saved and saved.get("argv"):
            if launch_with(saved["argv"], it.path):
                return
            # Saved launcher no longer works — forget it, then fall through.
            self._forget_assoc(ext)

        # 2. OS default handler.
        try:
            if default_open(it.path):
                return
        except Exception:
            traceback.print_exc()

        # 3. Our own Open-with dialog (also handles cfg["assoc"] writes).
        self.do_open_with()

    def open_current_folder(self):
        it = self._current_item()
        if it is not None and it.is_dir:
            self.navigate(it.path)

    def open_terminal(self, path=None):
        """Spawn a terminal emulator at `path` (defaults to self.cwd)."""
        if path is None:
            path = self.cwd
        if not path or path in VIRTUAL:
            self.flash("Cannot open a terminal here")
            return
        if IS_WIN:
            try:
                subprocess.Popen(
                    ["powershell.exe", "-NoExit", "-Command",
                     f"cd '{path}'"],
                    creationflags=getattr(
                        subprocess, "CREATE_NEW_CONSOLE", 0))
                return
            except OSError as err:
                self.flash(f"Could not open PowerShell: {err}")
                return
        if sys.platform == "darwin":
            for app in ("Terminal", "iTerm"):
                try:
                    subprocess.Popen(
                        ["open", "-a", app, path],
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL)
                    return
                except Exception:
                    continue
        # Linux / other: try common terminals in order.
        candidates = [
            ["gnome-terminal", "--working-directory", path],
            ["konsole", "--workdir", path],
            ["xfce4-terminal", "--working-directory", path],
            ["tilix", "--working-directory", path],
            ["terminator", "--working-directory", path],
            ["alacritty", "--working-directory", path],
            ["kitty", "--directory", path],
            ["xterm", "-e", f"cd '{path}' && exec $SHELL"],
        ]
        for cmd in candidates:
            try:
                subprocess.Popen(
                    cmd,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    start_new_session=True)
                return
            except FileNotFoundError:
                continue
            except Exception:
                continue
        self.flash("No terminal found")

    def do_open_with(self):
        """Open-with dialog for the current (or selected single) file."""
        items = self._selected_items()
        if not items:
            it = self._current_item()
            if it is not None:
                items = [it]
        if not items:
            self.flash("Nothing selected")
            return
        if len(items) > 1:
            self.flash("Select one file to open with…")
            return
        it = items[0]
        if it.is_dir:
            self.flash("Pick a file, not a folder")
            return
        if not os.path.isfile(it.path):
            self.flash("File does not exist")
            return

        saved, ext = self._assoc_for(it.name)
        dlg = OpenWithDialog(it.path, it.name, ext, saved, self)
        if dlg.exec() != QDialog.Accepted:
            return

        # Forget was requested — drop the entry and don't launch.
        if dlg.forget:
            assoc = self.cfg.get("assoc") or {}
            if ext in assoc:
                del assoc[ext]
                save_config(self.cfg)
                self.flash(f"Forgot saved app for {ext or 'files'}")
            return

        chosen = dlg.chosen
        if not chosen or not chosen.get("argv"):
            return

        # Persist if the user asked for it.
        if dlg.always and ext:
            self.cfg.setdefault("assoc", {})[ext] = {
                "name": chosen.get("name") or os.path.basename(
                    str(chosen["argv"][0])),
                "argv": list(chosen["argv"]),
            }
            save_config(self.cfg)
            self.flash(f"Saved “{self.cfg['assoc'][ext]['name']}” "
                       f"for {ext} files")

        if not launch_with(chosen["argv"], it.path):
            self.flash(f"Could not launch “{chosen.get('name') or 'app'}”")

    def copy_path(self):
        items = self._selected_items()
        if not items:
            it = self._current_item()
            if it is not None:
                items = [it]
        if not items:
            self.flash("Nothing selected")
            return
        text = "\n".join(it.path for it in items)
        QApplication.clipboard().setText(text)
        self.flash("Path copied")

    def open_menu_bar(self):
        mb = self.menuBar()
        acts = mb.actions()
        if acts:
            mb.setActiveAction(acts[0])

    # -------------------------------------------------- shortcut dialogs
    def capture_key(self, parent=None):
        dlg = _KeyCaptureDialog(parent or self)
        if dlg.exec() != QDialog.Accepted:
            return None
        if dlg.spec is None:
            return None
        return dlg.spec

    def _change_shortcut(self, aid):
        spec = self.capture_key(self)
        if spec is None:
            return
        if not spec:
            self.flash("No key captured")
            return
        spec = norm_spec(spec)
        if not spec:
            return
        if spec in RESERVED:
            QMessageBox.information(
                self, "Reserved shortcut",
                f"{spec} is a fixed shortcut and cannot be reassigned.")
            return
        if not is_safe_key(spec):
            QMessageBox.information(
                self, "Unsafe shortcut",
                "That key would be typed away in text fields.\n"
                "Use Ctrl+…, Alt+…, or an F-key.")
            return

        owner = None
        for other in ACTION_INFO:
            if other == aid:
                continue
            if spec in [norm_spec(s) for s in self._effective_keys(other)]:
                owner = other
                break
        if owner is not None:
            reply = QMessageBox.question(
                self, "Shortcut in use",
                f"{spec} is already bound to "
                f"“{ACTION_INFO[owner][1]}”.\n"
                f"Reassign it to “{ACTION_INFO[aid][1]}”?",
                QMessageBox.Yes | QMessageBox.No, QMessageBox.No)
            if reply != QMessageBox.Yes:
                return
            keys = self.cfg.setdefault("keys", {})
            keys[owner] = [s for s in self._effective_keys(owner)
                           if norm_spec(s) != spec]

        cur = [norm_spec(s) for s in self._effective_keys(aid)]
        if spec in cur:
            return
        cur.append(spec)
        self.cfg.setdefault("keys", {})[aid] = cur
        save_config(self.cfg)
        self._bind_shortcuts()

    def show_shortcuts(self):
        dlg = QDialog(self)
        dlg.setWindowTitle("Keyboard shortcuts")
        dlg.resize(720, 780)
        v = QVBoxLayout(dlg)
        v.setContentsMargins(12, 12, 12, 12)
        v.setSpacing(8)

        hint = QLabel(
            "Double-click an action to rebind it. "
            "“Change…” captures the next key press (Esc cancels there). "
            "“Clear” removes every binding from the action.")
        hint.setWordWrap(True)
        v.addWidget(hint)

        tree = QTreeWidget()
        tree.setHeaderLabels(["Action / fixed key", "Key(s)"])
        tree.setColumnWidth(0, 380)
        tree.setAlternatingRowColors(True)
        tree.setRootIsDecorated(True)
        tree.setUniformRowHeights(True)
        tree.setSelectionMode(QAbstractItemView.SingleSelection)
        v.addWidget(tree, 1)

        row = QHBoxLayout()
        btn_change = QPushButton("Change…")
        btn_clear = QPushButton("Clear")
        btn_reset = QPushButton("Reset all")
        btn_close = QPushButton("Close")
        row.addWidget(btn_change)
        row.addWidget(btn_clear)
        row.addWidget(btn_reset)
        row.addStretch(1)
        row.addWidget(btn_close)
        v.addLayout(row)

        def rebuild():
            tree.clear()
            cats = {}
            order = []
            for entry in ACTION_DEFS:
                aid, title, cat = entry[0], entry[1], entry[2]
                if cat not in cats:
                    cats[cat] = []
                    order.append(cat)
                cats[cat].append((aid, title))
            for cat in order:
                top = QTreeWidgetItem([cat, ""])
                f = top.font(0)
                f.setBold(True)
                top.setFont(0, f)
                top.setFlags(Qt.ItemIsEnabled)
                tree.addTopLevelItem(top)
                for aid, title in cats[cat]:
                    specs = self._effective_keys(aid)
                    txt = "   ".join(specs) if specs else "(unassigned)"
                    child = QTreeWidgetItem([title, txt])
                    child.setData(0, Qt.UserRole, aid)
                    top.addChild(child)
                top.setExpanded(True)

            top = QTreeWidgetItem(["Fixed keys (cannot be changed)", ""])
            f = top.font(0)
            f.setBold(True)
            top.setFont(0, f)
            top.setFlags(Qt.ItemIsEnabled)
            tree.addTopLevelItem(top)
            for k, desc in FIXED_KEYS:
                child = QTreeWidgetItem([k, desc])
                child.setFlags(Qt.ItemIsEnabled)
                top.addChild(child)
            top.setExpanded(True)

        rebuild()

        def current_aid():
            it = tree.currentItem()
            if it is None:
                return None
            return it.data(0, Qt.UserRole)

        def change():
            aid = current_aid()
            if aid is None:
                QMessageBox.information(
                    dlg, "Keyboard shortcuts",
                    "Select an action first.")
                return
            self._change_shortcut(aid)
            rebuild()

        def clear_binding():
            aid = current_aid()
            if aid is None:
                return
            self.cfg.setdefault("keys", {})[aid] = []
            save_config(self.cfg)
            self._bind_shortcuts()
            rebuild()

        def do_reset():
            if QMessageBox.question(
                    dlg, "Reset shortcuts",
                    "Restore all shortcuts to their defaults?",
                    QMessageBox.Yes | QMessageBox.No) == QMessageBox.Yes:
                self.reset_all()
                rebuild()

        def on_double(item, _col):
            if item is None:
                return
            aid = item.data(0, Qt.UserRole)
            if aid is None:
                return
            self._change_shortcut(aid)
            rebuild()

        btn_change.clicked.connect(change)
        btn_clear.clicked.connect(clear_binding)
        btn_reset.clicked.connect(do_reset)
        btn_close.clicked.connect(dlg.accept)
        tree.itemDoubleClicked.connect(on_double)
        dlg.exec()

    # ----------------------------------------------------------- headings
    def update_headings(self):
        if self._search_active:
            titles = SEARCH_TITLES
        elif self.cwd == THIS_PC:
            titles = PC_TITLES
        elif self.cwd == RECYCLE:
            titles = BIN_TITLES
        else:
            titles = TITLES
        cols = tuple(c for c in COLS if titles.get(c) is not None)
        self.model.set_layout(cols, titles)
        self._apply_column_sizing()
        self._update_sort_indicator()

    def _apply_column_sizing(self):
        h = self.view.horizontalHeader()
        for i, col in enumerate(self.model.columns):
            mode = (QHeaderView.Stretch if col == "name"
                    else QHeaderView.Interactive)
            h.setSectionResizeMode(i, mode)
            self.view.setColumnWidth(i, COL_WIDTHS.get(col, 100))

    def _update_sort_indicator(self):
        h = self.view.horizontalHeader()
        cols = self.model.columns
        if self.sort_col in cols:
            idx = cols.index(self.sort_col)
            order = (Qt.DescendingOrder if self.sort_reverse
                     else Qt.AscendingOrder)
            h.setSortIndicatorShown(True)
            h.setSortIndicator(idx, order)
        else:
            h.setSortIndicatorShown(False)

    def _header_clicked(self, section):
        cols = self.model.columns
        if 0 <= section < len(cols):
            self.set_sort(cols[section])

    def set_sort(self, col):
        if self.sort_col == col:
            self.sort_reverse = not self.sort_reverse
        else:
            self.sort_col = col
            self.sort_reverse = False
        self.apply_filter_and_sort()

    # ---------------------------------------------------------- navigate
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
        self.path_edit.setText(self.display_path(target))
        self._update_pin_button()
        if getattr(self, "sidebar", None) is not None:
            self.sidebar.sync_selection(target)
        self.save_tab()
        return True

    def reload(self):
        self.entries.clear()
        if self.cwd == THIS_PC:
            self.entries = scan_drives(quick=False)
        elif self.cwd == RECYCLE:
            self.entries = scan_trash()
        elif self.cwd and os.path.isdir(self.cwd):
            try:
                with os.scandir(self.cwd) as it:
                    for e in it:
                        hidden = e.name.startswith(".")
                        if IS_WIN:
                            try:
                                flags = e.stat(
                                    follow_symlinks=False).st_file_attributes
                                if flags & stat.FILE_ATTRIBUTE_HIDDEN:
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
                        self.entries.append(
                            Item(e.name, e.path, is_dir, size, mtime, hidden))
            except OSError as err:
                self.flash(f"Access error: {err}")

        self.apply_filter_and_sort()

    def _sort_items(self, rows):
        def sort_key(it):
            col = self.sort_col
            if col == "size":
                val = it.size
            elif col == "modified":
                val = it.mtime or 0
            elif col == "where":
                val = (it.orig or "").lower()
            else:
                val = it.name.lower()
            return (not it.is_dir, val)

        rows.sort(key=sort_key, reverse=self.sort_reverse)
        return rows

    def apply_filter_and_sort(self):
        query = self.filter_edit.text().strip()

        if self.cwd == THIS_PC and query:
            if self._search_active and self._search_query == query:
                self._render_search()
            else:
                self._start_search(query)
            return
        if self._search_active:
            self._cancel_search()
            self.update_headings()

        flt = query.lower()
        rows = [e for e in self.entries if flt in e.name.lower()]
        self._sort_items(rows)
        self.shown = rows
        self.model.set_rows(rows)
        self._update_sort_indicator()

        if rows:
            self.view.setCurrentIndex(self.model.index(0, 0))
            self.view.selectRow(0)
        self.update_status()

    # ------------------------------------------------ system-wide search
    def _start_search(self, query):
        self._cancel_search()
        self._search_active = True
        self._search_query = query
        self._search_results = []
        self._search_truncated = False
        self.shown = []
        self.model.set_rows([])
        self.update_headings()

        roots = [it.path for it in self.entries if it.is_dir]
        if not roots:
            roots = [d.path for d in scan_drives(quick=True)]
        if not roots:
            self._search_active = False
            self.update_status()
            return

        th = _SystemSearchThread(query, roots, self.show_hidden, self)
        th.batch.connect(self._on_search_batch)
        th.done.connect(self._on_search_done)
        self._search_thread = th
        self.update_status()
        th.start()

    def _cancel_search(self, wait=False):
        th = self._search_thread
        self._search_thread = None
        self._search_active = False
        self._search_query = ""
        self._search_results = []
        self._search_truncated = False
        if th is None:
            return
        for sig, slot in ((th.batch, self._on_search_batch),
                          (th.done, self._on_search_done)):
            try:
                sig.disconnect(slot)
            except (RuntimeError, TypeError):
                pass
        th.cancel()
        self._retired.append(th)
        th.finished.connect(lambda t=th: self._reap_thread(t))
        if wait:
            th.wait(3000)

    def _reap_thread(self, th):
        if th in self._retired:
            self._retired.remove(th)

    def _on_search_batch(self, items):
        if self.sender() is not self._search_thread:
            return
        self._search_results.extend(items)
        self._render_search()

    def _on_search_done(self):
        if self.sender() is not self._search_thread:
            return
        self._search_truncated = bool(getattr(self._search_thread,
                                              "truncated", False))
        self._render_search()

    def _render_search(self):
        rows = self._sort_items(list(self._search_results))
        self.shown = rows
        self.model.set_rows(rows)
        self._update_sort_indicator()
        self.update_status()

    def closeEvent(self, event):
        self._cancel_search(wait=True)
        for th in list(self._retired):
            th.wait(3000)
        # Any app-loader threads we spawned must finish before the process
        # exits (Qt aborts if a QThread is destroyed while running).
        for th in list(_APP_LOADERS):
            th.wait(3000)
        super().closeEvent(event)

    # ---------------------------------------------------------- filtering
    def on_filter_change(self, *_):
        if self._suppress_filter:
            return
        self._filter_timer.start()

    def _apply_filter_debounced(self):
        self.apply_filter_and_sort()

    def focus_path(self):
        self.path_edit.setFocus()
        self.path_edit.selectAll()

    def on_path_enter(self):
        target = self.path_edit.text().strip()
        low = target.lower()
        if low in ("this pc", "thispc", "computer"):
            target = THIS_PC
        elif low in ("recycle bin", "recyclebin", "trash"):
            target = RECYCLE
        if self.navigate(target):
            self.view.setFocus()

    def on_path_escape(self):
        if self.cwd:
            self.path_edit.setText(self.display_path(self.cwd))
        self.view.setFocus()

    def focus_filter(self):
        self.filter_edit.setFocus()
        self.filter_edit.selectAll()

    def on_filter_escape(self):
        self.filter_edit.clear()
        self.apply_filter_and_sort()
        self.view.setFocus()

    def focus_tree(self):
        self.view.setFocus()

    def toggle_sidebar_focus(self):
        if self.sidebar.hasFocus():
            self.view.setFocus()
            return
        self.sidebar.setFocus()
        if self.cwd:
            self.sidebar.sync_selection(self.cwd)

    # --------------------------------------------------------- navigation
    def go_parent(self):
        if self.cwd in VIRTUAL:
            return
        if not self.cwd:
            return
        parent = os.path.dirname(self.cwd)
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

    # ------------------------------------------------------------ tabs
    def _tab_label(self, t):
        if not t.cwd:
            return "Home"
        if t.cwd == THIS_PC:
            return "This PC"
        if t.cwd == RECYCLE:
            return "Recycle Bin"
        return os.path.basename(t.cwd.rstrip(os.sep)) or t.cwd

    def _rebuild_tab_strip(self):
        tb = self.tab_bar
        if tb is None:
            return
        self._switching_tab = True
        try:
            tb.blockSignals(True)
            while tb.count() > 0:
                tb.removeTab(0)
            for t in self.tabs:
                i = tb.addTab(self._tab_label(t))
                tb.setTabToolTip(i, t.cwd or "")
            if 0 <= self.cur < tb.count():
                tb.setCurrentIndex(self.cur)
        finally:
            tb.blockSignals(False)
            self._switching_tab = False

    def _update_current_tab_label(self):
        tb = self.tab_bar
        if tb is None:
            return
        if not (0 <= self.cur < tb.count()):
            return
        t = self.tabs[self.cur]
        tb.setTabText(self.cur, self._tab_label(t))
        tb.setTabToolTip(self.cur, t.cwd or "")

    def _sync_tab_bar_current(self):
        tb = self.tab_bar
        if tb is None:
            return
        if not (0 <= self.cur < tb.count()):
            return
        self._switching_tab = True
        try:
            tb.blockSignals(True)
            tb.setCurrentIndex(self.cur)
        finally:
            tb.blockSignals(False)
            self._switching_tab = False

    def save_tab(self):
        if not self.tabs or not (0 <= self.cur < len(self.tabs)):
            return
        t = self.tabs[self.cur]
        t.cwd = self.cwd
        t.back = list(self.back)
        t.fwd = list(self.fwd)
        t.sort_col = self.sort_col
        t.sort_reverse = self.sort_reverse
        t.filter = self.filter_edit.text()
        t.sel = [it.name for it in self._selected_items()]
        t.focus = self._focus_item_name()
        self._update_current_tab_label()

    def load_tab(self, t):
        self._cancel_search()
        self._suppress_filter = True
        try:
            self.filter_edit.setText(t.filter)
        finally:
            self._suppress_filter = False
        self.back = list(t.back)
        self.fwd = list(t.fwd)
        self.sort_col = t.sort_col
        self.sort_reverse = t.sort_reverse
        target = t.cwd or os.path.expanduser("~")
        if not self.navigate(target, record=False):
            self.navigate(os.path.expanduser("~"), record=False)
        self.restore_view(t.sel, t.focus)
        self.view.setFocus()

    def restore_view(self, sel, foc):
        sm = self.view.selectionModel()
        if sm is None:
            return
        names_sel = set(sel or [])
        qsel = QItemSelection()
        last_col = max(0, self.model.columnCount() - 1)
        focus_row = -1
        for r, it in enumerate(self.shown):
            if it.name in names_sel:
                qsel.select(self.model.index(r, 0),
                            self.model.index(r, last_col))
            if foc is not None and it.name == foc and focus_row < 0:
                focus_row = r
        if names_sel:
            sm.select(qsel,
                      QItemSelectionModel.ClearAndSelect
                      | QItemSelectionModel.Rows)
        if focus_row < 0 and self.shown:
            focus_row = 0
        if focus_row >= 0:
            idx = self.model.index(focus_row, 0)
            self.view.setCurrentIndex(idx)
            self.view.scrollTo(idx, QAbstractItemView.EnsureVisible)
        self.update_status()

    def new_tab(self, cwd=None):
        self.save_tab()
        t = Tab()
        t.cwd = cwd if cwd is not None else self.cwd
        t.sort_col = self.sort_col
        t.sort_reverse = self.sort_reverse
        t.filter = ""
        t.sel = []
        t.focus = None
        self.tabs.append(t)
        self.cur = len(self.tabs) - 1
        self._rebuild_tab_strip()
        self.load_tab(self.tabs[self.cur])

    def close_tab(self, index=None):
        if index is None:
            index = self.cur
        if not (0 <= index < len(self.tabs)):
            return
        if len(self.tabs) <= 1:
            self.flash("Cannot close the last tab")
            return
        self.save_tab()
        was_current = (index == self.cur)
        del self.tabs[index]
        if was_current:
            self.cur = min(index, len(self.tabs) - 1)
            self._rebuild_tab_strip()
            self.load_tab(self.tabs[self.cur])
        else:
            if self.cur > index:
                self.cur -= 1
            self._rebuild_tab_strip()

    def switch_tab(self, index):
        if not (0 <= index < len(self.tabs)):
            return
        if index == self.cur:
            return
        self.save_tab()
        self.cur = index
        self._sync_tab_bar_current()
        self.load_tab(self.tabs[self.cur])

    def next_tab(self):
        if len(self.tabs) <= 1:
            return
        self.switch_tab((self.cur + 1) % len(self.tabs))

    def prev_tab(self):
        if len(self.tabs) <= 1:
            return
        self.switch_tab((self.cur - 1) % len(self.tabs))

    def _on_new_tab_requested(self):
        self.new_tab()

    def _on_tab_switched(self, index):
        if self._switching_tab:
            return
        if not (0 <= index < len(self.tabs)):
            return
        if index == self.cur:
            return
        self.switch_tab(index)

    def _on_tab_close_requested(self, index):
        self.close_tab(index)

    # ---------------------------------------------- file operations (ops)
    def do_copy(self):
        items = self._selected_items()
        if not items:
            self.flash("Nothing selected")
            return
        self.clip = [(it.path, it.is_dir) for it in items
                     if os.path.lexists(it.path)]
        if not self.clip:
            self.flash("Nothing to copy")
            return
        self.clip_mode = "copy"
        try:
            QApplication.clipboard().setText(
                "\n".join(p for p, _ in self.clip))
        except Exception:
            pass
        self.flash(f"Copied {len(self.clip)} item(s)")

    def do_cut(self):
        items = self._selected_items()
        if not items:
            self.flash("Nothing selected")
            return
        self.clip = [(it.path, it.is_dir) for it in items
                     if os.path.lexists(it.path)]
        if not self.clip:
            self.flash("Nothing to cut")
            return
        self.clip_mode = "cut"
        try:
            QApplication.clipboard().setText(
                "\n".join(p for p, _ in self.clip))
        except Exception:
            pass
        self.flash(f"Cut {len(self.clip)} item(s)")

    def do_paste(self):
        if not self.clip:
            self.flash("Clipboard is empty")
            return
        if not self.cwd or self.cwd in VIRTUAL or not os.path.isdir(self.cwd):
            self.flash("Cannot paste here")
            return
        if self._search_active:
            self.flash("Cannot paste while searching")
            return
        target_dir = self.cwd
        created = []
        errors = []
        for src_path, is_dir in self.clip:
            if not os.path.lexists(src_path):
                continue
            src_root = os.path.dirname(src_path.rstrip(os.sep))
            if self.clip_mode == "cut" and same_loc(src_root, target_dir):
                continue
            name = os.path.basename(src_path.rstrip(os.sep)) or "item"
            dest = unique_path(target_dir, name, is_dir)
            try:
                if self.clip_mode == "cut":
                    shutil.move(src_path, dest)
                else:
                    if is_dir:
                        shutil.copytree(src_path, dest, symlinks=True)
                    else:
                        shutil.copy2(src_path, dest, follow_symlinks=False)
                created.append(os.path.basename(dest))
            except Exception as err:
                errors.append(f"{name}: {err}")
        if self.clip_mode == "cut" and not errors:
            self.clip = []
        self.reload()
        if created:
            self._select_by_name(created[-1])
        if errors:
            self.flash(errors[0])
        else:
            self.flash(f"Pasted {len(created)} item(s)")

    def do_rename(self):
        items = self._selected_items()
        if not items:
            self.flash("Select one item to rename")
            return
        if len(items) != 1:
            self.flash("Cannot rename multiple items")
            return
        if self.cwd in VIRTUAL:
            self.flash("Cannot rename here")
            return
        it = items[0]
        new_name, ok = QInputDialog.getText(
            self, "Rename", "New name:",
            QLineEdit.Normal, it.name)
        if not ok:
            return
        new_name = new_name.strip()
        if not new_name or new_name in (".", "..") \
                or "/" in new_name or "\\" in new_name:
            self.flash("Invalid name")
            return
        if new_name == it.name:
            return
        dest = os.path.join(os.path.dirname(it.path), new_name)
        if os.path.lexists(dest):
            self.flash(f"Already exists: {new_name}")
            return
        try:
            os.rename(it.path, dest)
        except OSError as err:
            self.flash(f"Could not rename: {err}")
            return
        self.reload()
        self._select_by_name(new_name)

    def do_delete(self, permanent=False):
        items = self._selected_items()
        if not items:
            self.flash("Nothing selected")
            return
        if self.cwd in VIRTUAL and not permanent:
            self.flash("Cannot delete here")
            return
        if permanent:
            names = ", ".join(it.name for it in items[:3])
            if len(items) > 3:
                names += ", …"
            if QMessageBox.question(
                    self, "Delete permanently",
                    f"Permanently delete {len(items)} item(s)?\n{names}",
                    QMessageBox.Yes | QMessageBox.No,
                    QMessageBox.No) != QMessageBox.Yes:
                return
        errors = []
        for it in items:
            if not os.path.lexists(it.path):
                continue
            try:
                if permanent:
                    remove_path(it.path)
                    if it.info:
                        try:
                            os.unlink(it.info)
                        except OSError:
                            pass
                else:
                    move_to_trash(it.path)
            except Exception as err:
                errors.append(f"{it.name}: {err}")
        self.reload()
        if errors:
            self.flash(errors[0])
        elif permanent:
            self.flash(f"Deleted {len(items)} item(s)")
        else:
            self.flash(f"Moved {len(items)} item(s) to Recycle Bin")

    def do_create_shortcut(self):
        """Create a .lnk (Windows) or symlink (POSIX) to the current item."""
        items = self._selected_items()
        if not items:
            it = self._current_item()
            if it is not None:
                items = [it]
        if not items:
            self.flash("Nothing selected")
            return
        if len(items) > 1:
            self.flash("Select one item to shorten")
            return
        it = items[0]
        if self.cwd in VIRTUAL or not os.path.isdir(self.cwd):
            self.flash("Cannot create a shortcut here")
            return

        # Name collisions: "<base> - Shortcut<ext>", regenerate with (N).
        if it.is_dir:
            stem, ext = it.name, ".lnk" if IS_WIN else ""
        else:
            if IS_WIN:
                stem = os.path.splitext(it.name)[0]
                ext = ".lnk"
            else:
                stem, ext = it.name, ".lnk"
        base = f"{stem} - Shortcut"
        candidate = f"{base}{ext}"
        n = 1
        while os.path.lexists(os.path.join(self.cwd, candidate)):
            candidate = f"{base} ({n}){ext}"
            n += 1
        dest = os.path.join(self.cwd, candidate)

        if IS_WIN:
            try:
                ps_src = it.path.replace("'", "''")
                ps_dst = dest.replace("'", "''")
                script = (
                    "$ws = New-Object -ComObject WScript.Shell; "
                    f"$sc = $ws.CreateShortcut('{ps_dst}'); "
                    f"$sc.TargetPath = '{ps_src}'; "
                    "if (Test-Path -LiteralPath '" + ps_src +
                    "' -PathType Container) { $sc.WindowStyle = 1 }; "
                    "$sc.Save()"
                )
                cp = subprocess.run(
                    ["powershell.exe", "-NoProfile", "-NonInteractive",
                     "-Command", script],
                    capture_output=True, timeout=15,
                    creationflags=getattr(
                        subprocess, "CREATE_NO_WINDOW", 0))
                if cp.returncode != 0:
                    err = (cp.stderr or b"").decode("utf-8", "replace")
                    self.flash(f"Shortcut creation failed: {err.strip() or cp.returncode}")
                    return
            except Exception as err:
                self.flash(f"Could not create shortcut: {err}")
                return
        else:
            try:
                os.symlink(it.path, dest)
            except OSError as err:
                self.flash(f"Could not create shortcut: {err}")
                return

        self.reload()
        self._select_by_name(candidate)
        self.flash(f"Created {candidate}")

    def do_new_file(self):
        if not self.cwd or self.cwd in VIRTUAL or not os.path.isdir(self.cwd):
            self.flash("Cannot create files here")
            return
        name, ok = QInputDialog.getText(
            self, "New file", "Name:", QLineEdit.Normal, "")
        if not ok:
            return
        name = name.strip()
        if not name or name in (".", "..") or "/" in name or "\\" in name:
            if name:
                self.flash("Invalid name")
            return
        dest = unique_path(self.cwd, name, False)
        try:
            with open(dest, "xb"):
                pass
        except OSError as err:
            self.flash(f"Could not create file: {err}")
            return
        self.reload()
        base = os.path.basename(dest)
        self._select_by_name(base)
        self.flash(f"Created {base}")

    def do_new_folder(self):
        if not self.cwd or self.cwd in VIRTUAL or not os.path.isdir(self.cwd):
            self.flash("Cannot create folders here")
            return
        name, ok = QInputDialog.getText(
            self, "New folder", "Name:", QLineEdit.Normal, "")
        if not ok:
            return
        name = name.strip()
        if not name or name in (".", "..") or "/" in name or "\\" in name:
            if name:
                self.flash("Invalid name")
            return
        dest = unique_path(self.cwd, name, True)
        try:
            os.makedirs(dest)
        except OSError as err:
            self.flash(f"Could not create folder: {err}")
            return
        self.reload()
        base = os.path.basename(dest)
        self._select_by_name(base)
        self.flash(f"Created {base}")

    def do_properties(self):
        items = self._selected_items()
        if not items:
            it = self._current_item()
            if it is None:
                self.flash("Nothing selected")
                return
            items = [it]
        self._show_properties(items)

    def _show_properties(self, items):
        if len(items) == 1:
            self._show_one_property(items[0])
        else:
            self._show_multi_properties(items)

    def _show_one_property(self, it):
        dlg = QDialog(self)
        dlg.setWindowTitle(f"{it.name} — Properties")
        dlg.resize(520, 380)
        v = QVBoxLayout(dlg)
        form = QFormLayout()
        form.setLabelAlignment(Qt.AlignRight)
        form.addRow("Name:", QLabel(it.name))
        form.addRow("Path:", QLabel(it.path))
        kind = "Folder" if it.is_dir else self._kind_label(it.name)
        form.addRow("Type:", QLabel(kind))
        if it.extra:
            form.addRow("Size:", QLabel(it.extra))
        elif it.is_dir:
            form.addRow("Size:", QLabel("—"))
        else:
            form.addRow("Size:",
                        QLabel(f"{human_size(it.size)} ({it.size} bytes)"))
        if it.mtime:
            form.addRow("Modified:", QLabel(fmt_time(it.mtime)))
        if it.orig:
            form.addRow("Original location:", QLabel(it.orig))
        # Saved association (step 7).
        if not it.is_dir:
            saved, ext = self._assoc_for(it.name)
            if saved and ext:
                form.addRow(
                    f"Opens with ({ext}):",
                    QLabel(f"{saved.get('name') or 'saved app'}"))
        try:
            st = os.stat(it.path, follow_symlinks=False)
            form.addRow("Permissions:",
                        QLabel(stat.filemode(st.st_mode)))
            if not IS_WIN:
                form.addRow("Owner UID:", QLabel(str(st.st_uid)))
                form.addRow("Group GID:", QLabel(str(st.st_gid)))
        except OSError:
            pass
        v.addLayout(form)
        v.addStretch(1)
        row = QHBoxLayout()
        row.addStretch(1)
        btn = QPushButton("Close")
        btn.clicked.connect(dlg.accept)
        row.addWidget(btn)
        v.addLayout(row)
        dlg.exec()

    def _show_multi_properties(self, items):
        dlg = QDialog(self)
        dlg.setWindowTitle(f"{len(items)} items — Properties")
        dlg.resize(520, 320)
        v = QVBoxLayout(dlg)
        form = QFormLayout()
        form.setLabelAlignment(Qt.AlignRight)
        form.addRow("Items:", QLabel(str(len(items))))
        total = 0
        for it in items:
            if not it.is_dir:
                total += it.size
        form.addRow("Total size:",
                    QLabel(f"{human_size(total)} ({total} bytes)"))
        v.addLayout(form)
        v.addStretch(1)
        row = QHBoxLayout()
        row.addStretch(1)
        btn = QPushButton("Close")
        btn.clicked.connect(dlg.accept)
        row.addWidget(btn)
        v.addLayout(row)
        dlg.exec()

    def do_restore(self):
        if self.cwd != RECYCLE:
            self.flash("Restore works in the Recycle Bin only")
            return
        items = self._selected_items()
        if not items:
            self.flash("Nothing selected")
            return
        errors = []
        restored = 0
        for it in items:
            if not os.path.lexists(it.path):
                continue
            target_root = ""
            if it.orig:
                target_root = os.path.dirname(it.orig.rstrip(os.sep))
            if not target_root or not os.path.isdir(target_root):
                target_root = os.path.expanduser("~")
            name = (os.path.basename(it.orig.rstrip(os.sep))
                    if it.orig else it.name) or it.name
            dest = unique_path(target_root, name, it.is_dir)
            try:
                shutil.move(it.path, dest)
            except Exception as err:
                errors.append(f"{it.name}: {err}")
                continue
            restored += 1
            if it.info:
                try:
                    os.unlink(it.info)
                except OSError:
                    pass
        self.reload()
        if errors:
            self.flash(errors[0])
        else:
            self.flash(f"Restored {restored} item(s)")

    def do_empty_bin(self):
        if QMessageBox.question(
                self, "Empty Recycle Bin",
                "Permanently delete everything in the Recycle Bin?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No) != QMessageBox.Yes:
            return
        if IS_WIN:
            try:
                win_empty_recycle_bin()
            except OSError as err:
                self.flash(f"Could not empty Recycle Bin: {err}")
                return
            if self.cwd == RECYCLE:
                self.reload()
            self.flash("Recycle Bin emptied")
            return
        items = scan_trash()
        if not items:
            self.flash("Recycle Bin is already empty")
            return
        errors = []
        for it in items:
            try:
                remove_path(it.path)
                if it.info:
                    try:
                        os.unlink(it.info)
                    except OSError:
                        pass
            except Exception as err:
                errors.append(str(err))
        if self.cwd == RECYCLE:
            self.reload()
        if errors:
            self.flash(errors[0])
        else:
            self.flash("Recycle Bin emptied")

    def do_context_menu(self):
        idx = self.view.currentIndex()
        if idx.isValid():
            rect = self.view.visualRect(idx)
            vp_pos = rect.center() if rect.isValid() else QPoint(8, 8)
        else:
            vp_pos = QPoint(8, 8)
        global_pos = self.view.viewport().mapToGlobal(vp_pos)
        self._on_list_context(self.view.mapFromGlobal(global_pos))

    # ------------------------------------------------------------ menus
    def _menu_item(self, menu, aid, label=None):
        if label is None:
            info = ACTION_INFO.get(aid) if aid else None
            label = info[1] if info else aid
        specs = self._effective_keys(aid) if aid else []
        short = specs[0] if specs else ""
        text = f"{label}\t{short}" if short else label
        act = menu.addAction(text)
        if aid:
            act.triggered.connect(
                lambda _checked=False, aid=aid: self._run_action(aid))
        return act

    def _on_list_context(self, pos):
        # --- Shift + Right-Click -> terminal at the current folder -----
        if QApplication.keyboardModifiers() & Qt.ShiftModifier:
            if self.cwd and self.cwd not in VIRTUAL:
                self.open_terminal(self.cwd)
            else:
                self.flash("Cannot open a terminal here")
            return

        # --- normal right-click: resolve target, build menu ------------
        global_pos = QCursor.pos()
        try:
            vp_pos = self.view.viewport().mapFromGlobal(global_pos)
            idx = self.view.indexAt(vp_pos)
        except Exception:
            idx = QModelIndex()

        if idx.isValid():
            sm = self.view.selectionModel()
            if sm is None or not sm.isRowSelected(idx.row(), QModelIndex()):
                self.view.clearSelection()
                self.view.setCurrentIndex(idx)
                self.view.selectRow(idx.row())
        else:
            self.view.clearSelection()

        items = self._selected_items()
        in_bin = (self.cwd == RECYCLE)
        in_pc = (self.cwd == THIS_PC)
        in_real = bool(self.cwd) and self.cwd not in VIRTUAL
        menu = QMenu(self.view)

        term_label = ("Open PowerShell here" if IS_WIN
                      else "Open Terminal here")

        if items and in_bin:
            self._menu_item(menu, "restore", "Restore")
            self._menu_item(menu, "delete_perm", "Delete permanently")
            menu.addSeparator()
            self._menu_item(menu, "properties")
            menu.addSeparator()
            self._menu_item(menu, "empty_bin", "Empty Recycle Bin")
        elif items and in_pc:
            self._menu_item(menu, "properties", "Drive properties")
            menu.addSeparator()
            self._menu_item(menu, "empty_bin", "Empty Recycle Bin")
            self._menu_item(menu, "refresh")
        elif items and in_real:
            self._menu_item(menu, "open")
            self._menu_item(menu, "open_with")
            menu.addSeparator()
            # Terminal at the current folder (Shift+RC does the same fast).
            menu.addAction(term_label,
                           lambda: self.open_terminal(self.cwd))
            menu.addSeparator()
            self._menu_item(menu, "cut")
            self._menu_item(menu, "copy")
            if self.clip:
                self._menu_item(menu, "paste")
            act_shortcut = menu.addAction(
                "Create shortcut", self.do_create_shortcut)
            menu.addSeparator()
            self._menu_item(menu, "delete")
            self._menu_item(menu, "rename")
            menu.addSeparator()
            self._menu_item(menu, "properties")
        elif in_real:
            menu.addAction(term_label,
                           lambda: self.open_terminal(self.cwd))
            if self.clip:
                menu.addSeparator()
                self._menu_item(menu, "paste")
            menu.addSeparator()
            self._menu_item(menu, "new_file")
            self._menu_item(menu, "new_folder")
            menu.addSeparator()
            self._menu_item(menu, "refresh")
        elif in_bin:
            self._menu_item(menu, "empty_bin", "Empty Recycle Bin")
            self._menu_item(menu, "refresh")
        elif in_pc:
            self._menu_item(menu, "refresh")

        if menu.actions():
            menu.exec(global_pos)

    # --------------------------------------------------------- sidebar
    def _populate_sidebar(self):
        sb = self.sidebar
        sb.clear()
        self._add_quick_access()
        self._add_this_pc()
        if self.cwd:
            sb.sync_selection(self.cwd)

    def _add_quick_access(self):
        sb = self.sidebar
        header = sb.make_section("QUICK ACCESS", self.palette["dim"])

        seen = []

        for path in self.cfg["pins"]:
            if path in VIRTUAL:
                continue
            if any(same_loc(path, s) for s in seen):
                continue
            name = os.path.basename(path.rstrip(os.sep)) or path
            sb.add_nav_item(header, name, path, tooltip=path)
            seen.append(path)

        for known_name, known_path in known_folders():
            if any(same_loc(known_path, s) for s in seen):
                continue
            if any(same_loc(known_path, up) for up in self.cfg["unpinned"]):
                continue
            tip = known_name if known_path in VIRTUAL else known_path
            sb.add_nav_item(header, known_name, known_path, tooltip=tip)
            seen.append(known_path)

        header.setExpanded(True)

    def _add_this_pc(self):
        sb = self.sidebar
        header = sb.make_section("THIS PC", self.palette["dim"],
                                 clickable=True, path=THIS_PC)

        drives = scan_drives(quick=True)
        for drive in drives:
            item = sb.add_nav_item(header, drive.name, drive.path,
                                   tooltip=drive.path)
            extra = self._drive_usage.get(drive.path)
            if extra:
                self._apply_usage_text(item, drive.path, extra)
            else:
                task = _DriveUsageTask(drive.path, self._usage_signals)
                QThreadPool.globalInstance().start(task)

        header.setExpanded(True)

    def _apply_usage_text(self, item, path, extra):
        m = _SIDEBAR_PCT_RE.search(extra)
        if not m:
            return
        pct = m.group(1)
        base = item.data(0, Qt.UserRole + 1)
        if not base:
            base = item.text(0)
            item.setData(0, Qt.UserRole + 1, base)
        item.setText(0, f"{base}  {pct}%")
        item.setToolTip(0, f"{path}\n{extra}")

    def _on_usage_ready(self, path, extra):
        self._drive_usage[path] = extra
        for i in range(self.sidebar.topLevelItemCount()):
            top = self.sidebar.topLevelItem(i)
            for j in range(top.childCount()):
                child = top.child(j)
                if child.data(0, Qt.UserRole) == path:
                    self._apply_usage_text(child, path, extra)
                    return

    def _on_sidebar_navigate(self, path):
        if self.navigate(path) and self.view.hasFocus():
            self.view.setFocus()

    def _on_sidebar_unpin(self, path):
        if path in VIRTUAL:
            self.flash("This location cannot be unpinned")
            return
        if not self.is_pinned(path):
            return
        self.unpin_folder(path)
        self.flash("Unpinned from Quick Access")

    def _on_sidebar_context(self, path, global_pos):
        # Shift + Right-Click -> terminal at the highlighted folder.
        if QApplication.keyboardModifiers() & Qt.ShiftModifier:
            if path and path not in VIRTUAL:
                self.open_terminal(path)
            else:
                self.flash("Cannot open a terminal here")
            return

        menu = QMenu(self)
        menu.addAction("Open", lambda: self.navigate(path))
        if path not in VIRTUAL:
            term_label = ("Open PowerShell here" if IS_WIN
                          else "Open Terminal here")
            menu.addSeparator()
            menu.addAction(term_label,
                           lambda p=path: self.open_terminal(p))
            menu.addSeparator()
            if self.is_pinned(path):
                menu.addAction("Unpin from Quick Access",
                               lambda: self._unpin_and_flash(path))
            else:
                menu.addAction("Pin to Quick Access",
                               lambda: self._pin_and_flash(path))
        menu.exec(QCursor.pos())

    def _pin_and_flash(self, path):
        self.pin_folder(path)
        self.flash("Pinned to Quick Access")

    def _unpin_and_flash(self, path):
        self.unpin_folder(path)
        self.flash("Unpinned from Quick Access")

    # ------------------------------------------- pin state (Quick Access)
    def _remove_matching(self, lst, path):
        matched = [p for p in lst if same_loc(p, path)]
        for p in matched:
            lst.remove(p)
        return bool(matched)

    def _matches_known(self, path):
        return any(same_loc(kp, path) for _, kp in known_folders())

    def is_pinned(self, path):
        if not path or path in VIRTUAL:
            return False
        if any(same_loc(p, path) for p in self.cfg["pins"]):
            return True
        if any(same_loc(p, path) for p in self.cfg["unpinned"]):
            return False
        return any(same_loc(kp, path) for _, kp in known_folders())

    def pin_folder(self, path):
        if not path or path in VIRTUAL:
            return
        if self.is_pinned(path):
            return
        if self._matches_known(path):
            self._remove_matching(self.cfg["unpinned"], path)
        elif not any(same_loc(p, path) for p in self.cfg["pins"]):
            self.cfg["pins"].append(path)
        save_config(self.cfg)
        self._populate_sidebar()
        self._update_pin_button()

    def unpin_folder(self, path):
        if not path or path in VIRTUAL:
            return
        removed = self._remove_matching(self.cfg["pins"], path)
        if not removed and self._matches_known(path):
            if not any(same_loc(p, path) for p in self.cfg["unpinned"]):
                self.cfg["unpinned"].append(path)
                removed = True
        if not removed:
            return
        save_config(self.cfg)
        self._populate_sidebar()
        self._update_pin_button()

    def toggle_pin_current(self):
        if not self.cwd:
            return
        if self.cwd in VIRTUAL:
            self.flash("This location cannot be pinned")
            return
        if self.is_pinned(self.cwd):
            self._unpin_and_flash(self.cwd)
        else:
            self._pin_and_flash(self.cwd)

    def _update_pin_button(self):
        P = self.palette
        if self.cwd and self.cwd not in VIRTUAL and self.is_pinned(self.cwd):
            color, tip = P["accent"], "Unpin this folder"
        else:
            color, tip = P["dim"], "Pin this folder"
        self.btn_pin.setText("Pin")
        self.btn_pin.setToolTip(tip)
        self.btn_pin.setStyleSheet(
            f"QToolButton {{ color: {color}; background: transparent; "
            f"border: none; padding: 4px 10px; font-weight: 600; }}"
        )

    # ----------------------------------------------------- status / flash
    def flash(self, text, ms=3000):
        self.status.setText(text)
        self._flash_timer.start(ms)

    def update_status(self):
        sel = 0
        sm = self.view.selectionModel()
        if sm is not None:
            sel = len(sm.selectedRows())

        if self._search_active:
            n = len(self._search_results)
            running = (self._search_thread is not None
                       and self._search_thread.isRunning())
            if running:
                text = f"Searching for “{self._search_query}”… {n} found"
            else:
                text = f"{n} results for “{self._search_query}”"
                if self._search_truncated:
                    text += " (stopped at limit)"
            if sel > 0:
                text += f" ({sel} selected)"
            self.status.setText(text)
            return

        total = len(self.shown)
        text = f"{total} items"
        if sel > 0:
            text += f" ({sel} selected)"
        self.status.setText(text)


if __name__ == "__main__":
    start_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~")
    FileManager(None, start_dir)
