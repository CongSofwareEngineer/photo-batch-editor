"""Input photo list with 64 px thumbnails generated in the background (spec section 8.2)."""

from __future__ import annotations

from pathlib import Path

from PySide6.QtCore import QPoint, QSize, Qt, QThreadPool, Signal
from PySide6.QtGui import QAction, QColor, QGuiApplication, QIcon, QImage, QPainter, QPixmap
from PySide6.QtWidgets import QListWidget, QListWidgetItem, QMenu, QWidget

from core.i18n import tr
from core.io_utils import open_in_file_manager
from core.system import IS_MAC
from ui.icons import icon
from ui.workers import Task, load_reduced

THUMB = 64
PATH_ROLE = Qt.ItemDataRole.UserRole + 1


def placeholder_pixmap(size: int, color: str = '#1b2640') -> QPixmap:
    pm = QPixmap(size, size)
    pm.fill(QColor(color))
    return pm


class FileList(QListWidget):
    """``current_path_changed`` carries the selected input file (or ``None``)."""

    current_path_changed = Signal(object)
    open_in_photo_editor = Signal(object)  # Path (right-click menu)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setIconSize(QSize(THUMB, THUMB))
        self.setUniformItemSizes(True)
        self.setMinimumWidth(200)
        self.setSpacing(1)
        self.pool = QThreadPool(self)
        self.pool.setMaxThreadCount(2)
        self._generation = 0
        self._placeholder = QIcon(placeholder_pixmap(THUMB))
        self.currentItemChanged.connect(self._current_changed)
        self.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self.customContextMenuRequested.connect(self._context_menu)

    def set_files(self, folder: Path | None, files: list[Path]) -> None:
        self._generation += 1
        self.pool.clear()
        self.clear()
        if folder is None:
            return
        gen = self._generation
        for i, f in enumerate(files):
            rel = f.relative_to(folder).as_posix()
            item = QListWidgetItem(self._placeholder, rel)
            item.setData(PATH_ROLE, f)
            item.setToolTip(str(f))
            self.addItem(item)
            Task.submit(
                self.pool,
                lambda f=f: load_reduced(f, THUMB, THUMB)[0],
                lambda img, i=i, g=gen: self._thumb_ready(g, i, img),
            )
        if files:
            self.setCurrentRow(0)

    def _thumb_ready(self, gen: int, row: int, img: QImage) -> None:
        if gen != self._generation:
            return
        item = self.item(row)
        if item is not None:  # centre in a square so the names line up
            pm = QPixmap(THUMB, THUMB)
            pm.fill(Qt.GlobalColor.transparent)
            painter = QPainter(pm)
            painter.drawImage((THUMB - img.width()) // 2, (THUMB - img.height()) // 2, img)
            painter.end()
            item.setIcon(QIcon(pm))

    def current_path(self) -> Path | None:
        item = self.currentItem()
        return item.data(PATH_ROLE) if item else None

    def _current_changed(self, cur: QListWidgetItem | None, _prev: QListWidgetItem | None) -> None:
        self.current_path_changed.emit(cur.data(PATH_ROLE) if cur else None)

    def _context_menu(self, pos: QPoint) -> None:
        item = self.itemAt(pos)
        if item is None:
            return
        path: Path = item.data(PATH_ROLE)
        menu = QMenu(self)
        a_edit = QAction(icon('image'), tr('Open in Photo editor'), menu)
        a_show = QAction(tr('Show in Finder') if IS_MAC else tr('Show in File Explorer'), menu)
        a_copy = QAction(tr('Copy path'), menu)
        for a in (a_edit, a_show, a_copy):
            menu.addAction(a)
        chosen = menu.exec(self.viewport().mapToGlobal(pos))
        if chosen is a_edit:
            self.open_in_photo_editor.emit(path)
        elif chosen is a_show:
            open_in_file_manager(path, select=True)
        elif chosen is a_copy:
            QGuiApplication.clipboard().setText(str(path))
