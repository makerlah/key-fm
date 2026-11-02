"""keyfm UI - PySide6 port.

Step 3 of the mandatory iteration protocol:
    path/filter shortcuts (Ctrl+L / Ctrl+F / Ctrl+B), typeahead, and the
    lasso (rubber-band selection) with faithful row-fraction edge auto-scroll.

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
STEP 3 (this file): path/filter shortcuts, typeahead, lasso (rubber-band
                    selection) with row-fraction edge auto-scroll.
STEP 4 (next):      Tab model save/load, tab strip widget, Alt+N, close.
STEP 5:             menus, ACTION_DEFS bindings, QShortcut, shortcuts dialog,
                    capture dialog, reset_all.
STEP 6:             context menus, rename / new file / new folder /
                    properties dialogs.
STEP 7:             open-with dialog, cfg["assoc"] persistence, Windows
                    chooser.
STEP 8:             apply_theme polish, both LIGHT and DARK.
"""
import os
import re
import shutil
import stat
import sys
import time
import traceback

from PySide6.QtCore import (
    Qt, QAbstractTableModel, QModelIndex, QObject, QRect, QRunnable,
    QThreadPool, QTimer, QItemSelection, QItemSelectionModel, Signal,
)
from PySide6.QtGui import (
    QColor, QPalette, QFont, QKeySequence, QShortcut,
)
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QTableView, QLineEdit, QLabel, QAbstractItemView, QHeaderView,
    QToolButton, QTreeWidget, QTreeWidgetItem, QSplitter, QMenu,
    QRubberBand,
)

from .constants import (
    APP, IS_WIN, THIS_PC, RECYCLE, VIRTUAL, COLS,
    TITLES, PC_TITLES, BIN_TITLES, QUICK_ICONS,
)
from .theme import LIGHT, DARK
from .models import Item, Tab
from .config import load_config, save_config
from .drives import scan_drives, known_folders
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

# Drive-usage "extra" text from scan_drives is shaped like
#   "12.3 GB used of 100.0 GB (12.3%)"
# so we can pull out the percentage cheaply when it arrives.
_SIDEBAR_PCT_RE = re.compile(r"\(([\d.]+)%\)")


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


# ===================================================== lasso + typeahead
class LassoTableView(QTableView):
    """QTableView with rubber-band lasso selection and keyboard typeahead.

    Lasso
    -----
    Press on empty space and drag: a rubber band follows the cursor. Every
    row whose viewport rectangle intersects the band is selected. Ctrl held
    at press time makes the lasso additive (existing selection is preserved).

    Edge auto-scroll uses the "row-fraction" math: for a pointer `p` px
    inside the EDGE_HOT band,

        frac = p / row_height_px        (in *rows*, not pixels)
        frac = min(frac, EDGE_MAX_FRAC) (clamp fast flings)

    ``frac`` is the number of rows-per-tick to scroll; a sub-row accumulator
    turns that into actual scrollbar moves so a shallow drag still moves the
    view smoothly instead of jerking one row at a time.

    Typeahead
    ---------
    Printable chars build a buffer that resets after TYPE_RESET_MS. The
    effective query is the buffer, except when it is all one character, in
    which case it is a single-char query that advances one hit per press.
    """

    DRAG_THRESHOLD = 4        # px before the rubber band appears
    EDGE_HOT       = 22       # px from an edge that starts auto-scroll
    EDGE_MAX_FRAC  = 3.0      # cap on rows scrolled per tick
    EDGE_TICK_MS   = 28       # ~35 ticks/sec — smooth but not too fast
    TYPE_RESET_MS  = 700

    def __init__(self, parent=None):
        super().__init__(parent)

        # --- lasso state ---------------------------------------------
        self._press_pt = None            # QPoint press origin (viewport)
        self._press_idx_row = -1
        self._press_mods = Qt.NoModifier
        self._lassoing = False
        self._lasso_mods = Qt.NoModifier
        self._base_selection = []        # rows selected when lasso began

        self._rubber = QRubberBand(QRubberBand.Rectangle, self.viewport())
        self._rubber.hide()

        # --- edge auto-scroll state ----------------------------------
        self._edge_timer = QTimer(self)
        self._edge_timer.setInterval(self.EDGE_TICK_MS)
        self._edge_timer.timeout.connect(self._edge_tick)
        self._scroll_dir = 0             # -1 up, +1 down, 0 none
        self._scroll_frac = 0.0          # rows per tick
        self._scroll_accum = 0.0

        # --- typeahead state -----------------------------------------
        self._type_buf = ""
        self._type_timer = QTimer(self)
        self._type_timer.setSingleShot(True)
        self._type_timer.setInterval(self.TYPE_RESET_MS)
        self._type_timer.timeout.connect(self._reset_type_buf)

    # ----------------------------------------------------- small helpers
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

    # ---------------------------------------------------------- mouse
    def mousePressEvent(self, event):
        if event.button() != Qt.LeftButton:
            super().mousePressEvent(event)
            return
        pt = event.position().toPoint()
        idx = self.indexAt(pt)
        if idx.isValid():
            # Row press -> normal Qt handling (click / modifier / drag-range).
            self._press_pt = None
            self._press_idx_row = -1
            super().mousePressEvent(event)
            return
        # Empty-space press: begin potential lasso.
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
            # Plain empty-space click -> deselect (already cleared on press).
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

    # ------------------------------------------------ edge auto-scroll
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

    # ------------------------------------------------------- typeahead
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

    # --------------------------------------------------- key handling
    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Escape and self._lassoing:
            self._finish_lasso()
            event.accept()
            return
        if self._handle_typeahead(event):
            event.accept()
            return
        super().keyPressEvent(event)


# =============================================================== Sidebar
class _SidebarUsageSignals(QObject):
    """Carrier for the drive-usage worker -> UI signal (Qt-friendly)."""
    ready = Signal(str, str)  # (drive_root, extra_text)


class _DriveUsageTask(QRunnable):
    """One-shot background task: compute ``shutil.disk_usage`` for one root.

    Runs on ``QThreadPool.globalInstance()``; never touches widgets, only
    emits a signal. If the signals owner is already gone (app is exiting)
    we swallow the RuntimeError raised by PySide on a dead emitter.
    """

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
            # Signals object was destroyed before the worker finished.
            pass


class Sidebar(QTreeWidget):
    """Left-hand nav: QUICK ACCESS (pins + known folders) and THIS PC.

    The widget itself stays dumb: it emits:
        * ``navigateTo(path)``         on click / Enter
        * ``escapePressed``            on Escape
        * ``unpinFromSidebar(path)``   on Delete
        * ``contextMenuRequested(path, global_pos)``

    and provides a handful of tiny helpers used by FileManager when it
    rebuilds the tree. All state (``cfg["pins"]`` / ``cfg["unpinned"]``)
    lives in FileManager; the sidebar never mutates it.
    """

    navigateTo = Signal(str)
    escapePressed = Signal()
    unpinFromSidebar = Signal(str)
    contextMenuRequested = Signal(str, object)  # path, QPoint (global)

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
        # itemAt(pos) wants viewport-relative coordinates, so the policy
        # goes on the viewport, not on the tree widget itself.
        self.viewport().setContextMenuPolicy(Qt.CustomContextMenu)
        self.viewport().customContextMenuRequested.connect(
            self._on_context_menu)
        self.itemClicked.connect(self._on_item_clicked)

    # --------------------------------------------------------- events
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
        # Esc -> back to the file list.
        if event.key() == Qt.Key_Escape:
            self.escapePressed.emit()
            return
        # Delete -> unpin the selected entry.
        if event.key() == Qt.Key_Delete:
            item = self.currentItem()
            if item is not None:
                path = item.data(0, Qt.UserRole)
                if path is not None:
                    self.unpinFromSidebar.emit(path)
                    return
        # Enter -> navigate to the selected entry.
        if event.key() in (Qt.Key_Return, Qt.Key_Enter):
            item = self.currentItem()
            if item is not None:
                path = item.data(0, Qt.UserRole)
                if path is not None:
                    self.navigateTo.emit(path)
                    return
        super().keyPressEvent(event)

    # --------------------------------------------------------- helpers
    def make_section(self, text, dim_color, clickable=False, path=None):
        """Add a bold section header. Non-clickable headers use NoItemFlags
        so they can't be selected or focused."""
        item = QTreeWidgetItem([text])
        if clickable:
            item.setFlags(Qt.ItemIsEnabled | Qt.ItemIsSelectable)
        else:
            item.setFlags(Qt.NoItemFlags)
        f = item.font(0)
        f.setBold(True)
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
        """Re-colour section headers (called from apply_theme)."""
        for i in range(self.topLevelItemCount()):
            top = self.topLevelItem(i)
            top.setForeground(0, QColor(dim_color))

    def sync_selection(self, path):
        """Highlight `path` if present; clear selection otherwise."""
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

        # Drive usage cache: path -> "X GB used of Y GB (Z%)".
        # Populated by background workers, kept across sidebar rebuilds.
        self._drive_usage = {}
        self._usage_signals = None    # created below, after _build_ui

        # Flash timer (status-bar override), replaced by QTimer.
        self._flash_timer = QTimer(self)
        self._flash_timer.setSingleShot(True)
        self._flash_timer.timeout.connect(self.update_status)

        # Filter debounce timer (fix vs Tk: Tk re-sorted on every keystroke).
        self._filter_timer = QTimer(self)
        self._filter_timer.setSingleShot(True)
        self._filter_timer.setInterval(150)
        self._filter_timer.timeout.connect(self._apply_filter_debounced)

        # Tabs are stubbed at step 3; real switch arrives in step 4.
        self.tabs = [Tab()]
        self.cur = 0

        # ---- window --------------------------------------------------
        self.setWindowTitle(APP)
        self.resize(1180, 700)
        self.setMinimumSize(760, 420)

        self._build_ui()
        self.apply_theme()

        # ---- sidebar data (needs self.palette to be set) -------------
        self._usage_signals = _SidebarUsageSignals(self)
        self._usage_signals.ready.connect(self._on_usage_ready)
        self._populate_sidebar()

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

        self.btn_pin = self._flat_btn("☆", self.toggle_pin_current)
        top.addWidget(self.btn_pin)

        top.addWidget(QLabel("🔍"))
        self.filter_edit = _FilterEdit()
        self.filter_edit.setMaximumWidth(220)
        self.filter_edit.textChanged.connect(self.on_filter_change)
        self.filter_edit.escapePressed.connect(self.on_filter_escape)
        self.filter_edit.downPressed.connect(self.focus_tree)
        top.addWidget(self.filter_edit)

        v.addLayout(top)

        # --- sidebar + file list (splitter) ----------------------------
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

        # --- path/filter/sidebar shortcuts -----------------------------
        # Step 5 will replace these with ACTION_DEFS-driven QShortcuts; for
        # now they are the only key bindings the app installs globally.
        QShortcut(QKeySequence("Ctrl+L"), self, self.focus_path)
        QShortcut(QKeySequence("Ctrl+F"), self, self.focus_filter)
        QShortcut(QKeySequence("Ctrl+B"), self, self.toggle_sidebar_focus)

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
            QTreeWidget#Sidebar {{
                background: {P['side']};
                color: {P['fg']};
                border: 1px solid {P['border']};
                border-radius: 4px;
                outline: none;
            }}
            QTreeWidget#Sidebar::item {{
                padding: 5px 6px;
                border-radius: 3px;
            }}
            QTreeWidget#Sidebar::item:hover {{
                background: {P['hover']};
            }}
            QTreeWidget#Sidebar::item:selected {{
                background: {P['sel_bg']};
                color: {P['sel_fg']};
            }}
            QSplitter::handle {{
                background: {P['bg']};
            }}
            QSplitter::handle:hover {{
                background: {P['hover']};
            }}
        """)

        # Section headers were coloured explicitly (setForeground). On a
        # theme switch we need to re-apply that colour with the new palette.
        if getattr(self, "sidebar", None) is not None:
            self.sidebar.apply_header_colors(P["dim"])

        # ★ pin button reflects current location's pin state.
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
        # Keep the sidebar in sync so it reflects the current location.
        if getattr(self, "sidebar", None) is not None:
            self.sidebar.sync_selection(target)
        # save_tab is a stub at step 3; wired in step 4.
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
        # Rebuild immediately; the debounced pass (also queued by clear())
        # is a harmless no-op because set_rows dedupes identical row lists.
        self.apply_filter_and_sort()
        self.view.setFocus()

    def focus_tree(self):
        self.view.setFocus()

    def toggle_sidebar_focus(self):
        """Ctrl+B: jump to the sidebar, and back when already there."""
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

    # --------------------------------------------------------- sidebar
    def _populate_sidebar(self):
        """Rebuild the sidebar tree from cfg + known folders + drives."""
        sb = self.sidebar
        sb.clear()
        self._add_quick_access()
        self._add_this_pc()
        if self.cwd:
            sb.sync_selection(self.cwd)

    def _add_quick_access(self):
        sb = self.sidebar
        header = sb.make_section("QUICK ACCESS", self.palette["dim"])

        seen = []                       # list of paths already emitted

        # 1. User pins, in cfg order, deduped by same_loc.
        for path in self.cfg["pins"]:
            if path in VIRTUAL:
                continue
            if any(same_loc(path, s) for s in seen):
                continue
            name = os.path.basename(path.rstrip(os.sep)) or path
            icon = self._icon_for(path, name)
            sb.add_nav_item(header, f"{icon} {name}", path, tooltip=path)
            seen.append(path)

        # 2. Known folders that aren't already shown and aren't unpinned.
        for known_name, known_path in known_folders():
            if any(same_loc(known_path, s) for s in seen):
                continue
            if any(same_loc(known_path, up) for up in self.cfg["unpinned"]):
                continue
            icon = QUICK_ICONS.get(known_name, "📁")
            tip = known_name if known_path in VIRTUAL else known_path
            sb.add_nav_item(header, f"{icon} {known_name}", known_path,
                            tooltip=tip)
            seen.append(known_path)

        header.setExpanded(True)

    def _add_this_pc(self):
        sb = self.sidebar
        # Clickable "THIS PC" header that navigates to the virtual drives view.
        header = sb.make_section("THIS PC", self.palette["dim"],
                                 clickable=True, path=THIS_PC)

        drives = scan_drives(quick=True)
        for drive in drives:
            item = sb.add_nav_item(header, f"💾 {drive.name}", drive.path,
                                   tooltip=drive.path)
            # Re-apply cached usage if any, else spawn a worker.
            extra = self._drive_usage.get(drive.path)
            if extra:
                self._apply_usage_text(item, drive.path, extra)
            else:
                task = _DriveUsageTask(drive.path, self._usage_signals)
                QThreadPool.globalInstance().start(task)

        header.setExpanded(True)

    def _icon_for(self, path, name):
        """Best-effort icon: known folder name first, then by known path."""
        if name in QUICK_ICONS:
            return QUICK_ICONS[name]
        for known_name, known_path in known_folders():
            if same_loc(known_path, path):
                return QUICK_ICONS.get(known_name, "📁")
        return "📁"

    def _apply_usage_text(self, item, path, extra):
        """Append "  NN.N%" to a drive item and stash the full text in the
        tooltip. Called both when a worker finishes and on sidebar rebuild."""
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
        """Slot for the background disk-usage worker."""
        self._drive_usage[path] = extra
        # Find the drive item with this path and update it in place.
        for i in range(self.sidebar.topLevelItemCount()):
            top = self.sidebar.topLevelItem(i)
            for j in range(top.childCount()):
                child = top.child(j)
                if child.data(0, Qt.UserRole) == path:
                    self._apply_usage_text(child, path, extra)
                    return

    def _on_sidebar_navigate(self, path):
        # Sidebar click / Enter — mirror what the main list would do.
        if self.navigate(path) and self.view.hasFocus():
            # only pull focus back when we're in "keyboard mode" already
            self.view.setFocus()

    def _on_sidebar_unpin(self, path):
        if path in VIRTUAL:
            self.flash("This location cannot be unpinned")
            return
        if not self.is_pinned(path):
            # The Delete key is a no-op for non-pinned entries.
            return
        self.unpin_folder(path)
        self.flash("Unpinned from Quick Access")

    def _on_sidebar_context(self, path, global_pos):
        menu = QMenu(self)
        menu.addAction("Open", lambda: self.navigate(path))
        if path not in VIRTUAL:
            menu.addSeparator()
            if self.is_pinned(path):
                menu.addAction("Unpin from Quick Access",
                               lambda: self._unpin_and_flash(path))
            else:
                menu.addAction("Pin to Quick Access",
                               lambda: self._pin_and_flash(path))
        menu.exec(global_pos)

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
        """True when `path` is currently shown under Quick Access."""
        if not path or path in VIRTUAL:
            return False
        if any(same_loc(p, path) for p in self.cfg["pins"]):
            return True
        if any(same_loc(p, path) for p in self.cfg["unpinned"]):
            return False
        # Default: all default known folders start out pinned.
        return any(same_loc(kp, path) for _, kp in known_folders())

    def pin_folder(self, path):
        if not path or path in VIRTUAL:
            return
        if self.is_pinned(path):
            return
        if self._matches_known(path):
            # Re-enable a default folder: just drop it from unpinned.
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
        if not self.cwd or self.cwd in VIRTUAL:
            text, color = "☆", P["dim"]         # not pinnable here
        elif self.is_pinned(self.cwd):
            text, color = "★", P["accent"]      # currently pinned
        else:
            text, color = "☆", P["dim"]         # pinnable but not pinned
        self.btn_pin.setText(text)
        self.btn_pin.setStyleSheet(
            f"QToolButton {{ color: {color}; background: transparent; "
            f"border: none; padding: 2px 8px; }}"
        )

    # ------------------------------------------------------- tabs (stub)
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
