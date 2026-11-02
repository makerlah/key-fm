"""keyfm UI - PySide6 port.

Step 1 of the mandatory iteration protocol:
    window skeleton + QTableView + model + navigate / reload / sort / filter.

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
STEP 1 (this file):
    window, QTableView over ``self.shown``, FileListModel, navigate,
    reload, sort (header click), filter (150 ms debounce), path entry,
    status / flash, apply_theme (initial pass).
STEP 2 (next):      sidebar, QUICK ACCESS pins, THIS PC, drive usage threads,
                    sidebar context menu, Delete-to-unpin, ★ button.
STEP 3:             lasso geometry port, typeahead, filter/path shortcuts.
STEP 4:             Tab model save/load, tab strip widget, Alt+N, close.
STEP 5:             menus, ACTION_DEFS bindings, QShortcut, shortcuts dialog,
                    capture dialog, reset_all.
STEP 6:             context menus, rename / new file / new folder /
                    properties dialogs.
STEP 7:             open-with dialog, cfg["assoc"] persistence, Windows
                    chooser.
STEP 8:             apply_theme polish, both LIGHT and DARK.
"""
import os
import stat
import sys
import time
import traceback

from PySide6.QtCore import (
    Qt, QAbstractTableModel, QModelIndex, QTimer, Signal,
)
from PySide6.QtGui import QColor, QPalette, QFont
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QTableView, QLineEdit, QLabel, QAbstractItemView, QHeaderView,
    QToolButton,
)

from .constants import (
    APP, IS_WIN, THIS_PC, RECYCLE, VIRTUAL, COLS,
    TITLES, PC_TITLES, BIN_TITLES,
)
from .theme import LIGHT, DARK
from .models import Item, Tab
from .config import load_config, save_config
from .drives import scan_drives
from .trash import scan_trash
from .helpers import (
    human_size, fmt_time, same_loc, exists_loc,
    # NOTE: helpers.brk and helpers.set_clipboard are Tk-coupled and MUST
    # NOT be imported (per task instructions). set_clipboard will be
    # reimplemented locally when step 5/6 need the clipboard. brk does not
    # apply to Qt.
)


# Default widths (px) used by the file list.
COL_WIDTHS = {"name": 460, "where": 260, "size": 100, "modified": 160}

# Column-name -> alignment. Only "size" is right-aligned.
COL_ALIGN = {"size": Qt.AlignRight | Qt.AlignVCenter}


class FileListModel(QAbstractTableModel):
    """Thin table model over ``FileManager.shown``.

    ``set_rows`` is a no-op when the passed list contains the same Item
    objects in the same order, so re-applying the filter with no visible
    change (or re-sorting while only the tie-breaker changed) does not
    reset the view. That is what keeps 10k rows usable.
    """

    def __init__(self, parent=None):
        super().__init__(parent)
        self.rows = []
        self.columns = tuple(COLS)          # ("name","where","size","modified")
        self.titles = dict(TITLES)

    # ------------------------------------------------------ data setters
    def set_rows(self, rows):
        if len(rows) == len(self.rows) and all(
                a is b for a, b in zip(rows, self.rows)):
            return
        self.beginResetModel()
        self.rows = list(rows)
        self.endResetModel()

    def set_layout(self, columns, titles):
        """Swap which columns are shown and their header text."""
        columns = tuple(columns)
        if columns == self.columns and titles == self.titles:
            return
        self.beginResetModel()
        self.columns = columns
        self.titles = dict(titles)
        self.endResetModel()

    # --------------------------------------------------- Qt model API
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
                prefix = "📁 " if row.is_dir else "📄 "
                return prefix + row.name
            if col == "where":
                return row.orig or ""
            if col == "size":
                if row.is_dir:
                    return ""
                if row.extra:          # drives / recycle bin use `extra`
                    return row.extra
                return human_size(row.size)
            if col == "modified":
                return fmt_time(row.mtime) if row.mtime else ""
        elif role == Qt.TextAlignmentRole:
            if col == "size":
                return int(Qt.AlignRight | Qt.AlignVCenter)
            return int(Qt.AlignLeft | Qt.AlignVCenter)
        return None


class _PathEdit(QLineEdit):
    """QLineEdit that emits a signal for Escape (Qt has no such signal)."""
    escapePressed = Signal()

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape:
            self.escapePressed.emit()
            return
        super().keyPressEvent(event)


class _FilterEdit(QLineEdit):
    """QLineEdit that emits Escape (clear) and Down (jump to list)."""
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


class FileManager(QMainWindow):
    def __init__(self, root, start):
        # ---- Tk shim --------------------------------------------------
        # __main__.py hands us a tk.Tk(); withdraw it and schedule its
        # destruction for the first tick of Tk's own mainloop (which runs
        # after this constructor returns, because we block on Qt's exec).
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
        self.entries = []          # unfiltered
        self.shown = []            # filtered + sorted  (row == index here)
        self.back = []
        self.fwd = []
        self.sort_col = "name"
        self.sort_reverse = False
        self.clip = []
        self.clip_mode = "copy"
        self._suppress_filter = False

        # Flash timer (status-bar override), replaced by QTimer.
        self._flash_timer = QTimer(self)
        self._flash_timer.setSingleShot(True)
        self._flash_timer.timeout.connect(self.update_status)

        # Filter debounce timer (fix vs Tk: Tk re-sorted on every keystroke).
        self._filter_timer = QTimer(self)
        self._filter_timer.setSingleShot(True)
        self._filter_timer.setInterval(150)
        self._filter_timer.timeout.connect(self._apply_filter_debounced)

        # Tabs are stubbed at step 1; real switch arrives in step 4.
        self.tabs = [Tab()]
        self.cur = 0

        # ---- window --------------------------------------------------
        self.setWindowTitle(APP)
        self.resize(1180, 700)
        self.setMinimumSize(760, 420)

        self._build_ui()
        self.apply_theme()

        # Initial navigation. __main__.py already validated `start`, but
        # be defensive here so unit-style imports never crash.
        home = os.path.expanduser("~")
        if not self.navigate(start, record=False):
            self.navigate(home, record=False)

        # ---- block on Qt's event loop --------------------------------
        # When the last window closes, exec() returns and so does __init__.
        # __main__.py's `root.mainloop()` then runs, hits the scheduled
        # root.destroy, and exits immediately.
        self.show()
        self.app.exec()

    # ------------------------------------------------------------- UI build
    def _flat_btn(self, text, cmd):
        """Small flat toolbar button (QToolButton so the stylesheet can
        give it a transparent background on hover)."""
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
        v.setContentsMargins(8, 8, 8, 6)
        v.setSpacing(6)

        # --- toolbar row ------------------------------------------------
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

        self.btn_pin = self._flat_btn("★", self.toggle_pin_current)
        top.addWidget(self.btn_pin)

        top.addWidget(QLabel("🔍"))
        self.filter_edit = _FilterEdit()
        self.filter_edit.setMaximumWidth(220)
        self.filter_edit.textChanged.connect(self.on_filter_change)
        self.filter_edit.escapePressed.connect(self.on_filter_escape)
        self.filter_edit.downPressed.connect(self.focus_tree)
        top.addWidget(self.filter_edit)

        v.addLayout(top)

        # --- file list --------------------------------------------------
        self.model = FileListModel(self)
        self.view = QTableView()
        self.view.setModel(self.model)
        self.view.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.view.setSelectionMode(QAbstractItemView.ExtendedSelection)
        self.view.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.view.setShowGrid(False)
        self.view.setSortingEnabled(False)      # we sort ourselves
        self.view.verticalHeader().setVisible(False)
        self.view.setWordWrap(False)

        h = self.view.horizontalHeader()
        h.setHighlightSections(False)
        h.setSectionsClickable(True)
        h.setSectionResizeMode(QHeaderView.Interactive)
        h.setStretchLastSection(False)
        h.sectionClicked.connect(self._header_clicked)

        self.view.selectionModel().selectionChanged.connect(
            lambda *_: self.update_status())

        v.addWidget(self.view, 1)

        # --- status bar -------------------------------------------------
        self.status = QLabel("")
        self.status.setObjectName("StatusLabel")
        v.addWidget(self.status)

    # ----------------------------------------------------------- theme
    def apply_theme(self):
        """Initial Qt palette + stylesheet. Final polish lands in step 8."""
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
                border-radius: 4px;
                padding: 2px 8px;
            }}
            QToolButton:hover {{ background: {P['hover']}; }}
            QTableView {{
                background: {P['field']};
                color: {P['fg']};
                selection-background-color: {P['sel_bg']};
                selection-color: {P['sel_fg']};
                gridline-color: {P['border']};
                border: 1px solid {P['border']};
            }}
            QHeaderView::section {{
                background: {P['head']};
                color: {P['dim']};
                border: none;
                border-bottom: 1px solid {P['border']};
                padding: 6px 8px;
            }}
            QHeaderView::section:hover {{ background: {P['hover']}; }}
            QLineEdit {{
                background: {P['entry']};
                color: {P['fg']};
                border: 1px solid {P['border']};
                border-radius: 4px;
                padding: 4px 8px;
            }}
            QLineEdit:focus {{ border: 1px solid {P['accent']}; }}
            QLabel#StatusLabel {{
                background: {P['side']};
                color: {P['dim']};
                padding: 6px 14px;
                border-radius: 3px;
            }}
        """)

        # ★ pin button reflects state (always dim at step 1).
        self._update_pin_button()

    def toggle_dark(self):
        # Step 1 port of the small dark/light toggle; the View menu binding
        # arrives at step 5. Kept here so future steps reuse it verbatim.
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

    # ----------------------------------------------------------- headings
    def update_headings(self):
        titles = (PC_TITLES if self.cwd == THIS_PC
                  else BIN_TITLES if self.cwd == RECYCLE
                  else TITLES)
        cols = tuple(c for c in COLS if titles.get(c) is not None)
        self.model.set_layout(cols, titles)
        self._apply_column_sizing()
        self._update_sort_indicator()

    def _apply_column_sizing(self):
        h = self.view.horizontalHeader()
        for i, col in enumerate(self.model.columns):
            mode = QHeaderView.Stretch if col == "name" else QHeaderView.Interactive
            h.setSectionResizeMode(i, mode)
            self.view.setColumnWidth(i, COL_WIDTHS.get(col, 100))

    def _update_sort_indicator(self):
        h = self.view.horizontalHeader()
        cols = self.model.columns
        if self.sort_col in cols:
            idx = cols.index(self.sort_col)
            order = Qt.DescendingOrder if self.sort_reverse else Qt.AscendingOrder
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
        # save_tab is a stub at step 1; wired in step 4.
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

    def apply_filter_and_sort(self):
        flt = self.filter_edit.text().lower()
        rows = [e for e in self.entries if flt in e.name.lower()]

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
            # Folders-first grouping: reverse=True moves files first, which
            # matches the Tk behaviour (res.sort(..., reverse=...)).
            return (not it.is_dir, val)

        rows.sort(key=sort_key, reverse=self.sort_reverse)
        self.shown = rows
        self.model.set_rows(rows)
        self._update_sort_indicator()

        if rows:
            self.view.setCurrentIndex(self.model.index(0, 0))
            self.view.selectRow(0)
        self.update_status()

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
        self.view.setFocus()

    def focus_tree(self):
        self.view.setFocus()

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

    # ------------------------------------------------------------- stubs
    # These exist at step 1 so the toolbar buttons are wired; real behaviour
    # arrives with the sidebar (step 2) and tabs (step 4).
    def is_pinned(self, path):
        return False

    def toggle_pin_current(self):
        self.flash("Pinning arrives in a later step")

    def _update_pin_button(self):
        P = self.palette
        self.btn_pin.setStyleSheet(
            f"QToolButton {{ color: {P['dim']}; background: transparent; "
            f"border: none; padding: 2px 8px; }}"
        )

    def save_tab(self):
        # TODO step 4
        pass

    def load_tab(self, t):
        # TODO step 4
        pass

    def restore_view(self, sel, foc):
        # TODO step 4
        pass

    # ----------------------------------------------------- status / flash
    def flash(self, text, ms=3000):
        self.status.setText(text)
        self._flash_timer.start(ms)

    def update_status(self):
        total = len(self.shown)
        sel = 0
        sm = self.view.selectionModel()
        if sm is not None:
            sel = len(sm.selectedRows())
        text = f"{total} items"
        if sel > 0:
            text += f" ({sel} selected)"
        self.status.setText(text)


if __name__ == "__main__":
    # Fallback for `python -m keyfm.ui`; __main__.py is the normal entry.
    start_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~")
    FileManager(None, start_dir)
