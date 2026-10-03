"""Screen 3 — Results: list of edited photos (spec section 8.5)."""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any, cast

from PySide6.QtCore import (
    QAbstractTableModel,
    QModelIndex,
    QPersistentModelIndex,
    QSize,
    QSortFilterProxyModel,
    Qt,
    QThreadPool,
    Signal,
)
from PySide6.QtGui import QAction, QColor, QGuiApplication, QImage, QKeyEvent, QPixmap
from PySide6.QtWidgets import (
    QAbstractItemView,
    QButtonGroup,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QMenu,
    QMessageBox,
    QPushButton,
    QTableView,
    QVBoxLayout,
    QWidget,
)

from core.batch import BatchReport, FileResult
from core.i18n import tr, tr_msg
from core.io_utils import open_in_file_manager, open_with_default_app
from core.system import IS_MAC
from ui.notifier import Banner
from ui.widgets import button
from ui.workers import Task, load_reduced

log = logging.getLogger(__name__)

THUMB = 120
COLUMNS = ('Photo', 'File name', 'Sub-folder', 'Size', 'Status')
SORT_ROLE = Qt.ItemDataRole.UserRole + 10
RESULT_ROLE = Qt.ItemDataRole.UserRole + 11
STATUS_TEXT = {
    'ok': '✓ OK',
    'warning': '⚠',
    'error': '✗',
    'cancelled': '■ Cancelled',
}  # translated when shown
STATUS_COLOR = {'ok': '#34d399', 'warning': '#fbbf24', 'error': '#f87171', 'cancelled': '#8d9bb8'}


def human_size(n: int | None) -> str:
    if not n:
        return '—'
    if n >= 1 << 20:
        return f'{n / (1 << 20):.1f} MB'
    return f'{n / 1024:.0f} KB'


def status_text(r: FileResult) -> str:
    if r.status in ('ok', 'cancelled'):
        return tr(STATUS_TEXT[r.status])
    mark = STATUS_TEXT[r.status]
    return f'{mark} {tr_msg(r.message)}' if r.message else mark


def is_success(r: FileResult) -> bool:
    return r.status in ('ok', 'warning') and r.output_path is not None


class ResultsModel(QAbstractTableModel):
    """One row per input photo, from the BatchReport (the folder is not rescanned)."""

    def __init__(self, parent: Any = None) -> None:
        super().__init__(parent)
        self.rows: list[FileResult] = []
        self.thumbs: dict[int, QPixmap] = {}
        self._requested: set[int] = set()
        self._generation = 0
        self.pool = QThreadPool(self)
        self.pool.setMaxThreadCount(3)
        self._placeholder = QPixmap(THUMB, int(THUMB * 2 / 3))
        self._placeholder.fill(QColor('#1b2640'))

    def set_report(self, report: BatchReport | None) -> None:
        self.beginResetModel()
        self._generation += 1
        self.pool.clear()
        self.rows = list(report.files) if report else []
        self.thumbs.clear()
        self._requested.clear()
        self.endResetModel()

    def rowCount(self, parent: QModelIndex | QPersistentModelIndex = QModelIndex()) -> int:  # noqa: N802, B008
        return 0 if parent.isValid() else len(self.rows)

    def columnCount(self, parent: QModelIndex | QPersistentModelIndex = QModelIndex()) -> int:  # noqa: N802, B008
        return 0 if parent.isValid() else len(COLUMNS)

    def headerData(
        self, section: int, orientation: Qt.Orientation, role: int = Qt.ItemDataRole.DisplayRole
    ) -> Any:  # noqa: N802
        if orientation == Qt.Orientation.Horizontal and role == Qt.ItemDataRole.DisplayRole:
            return tr(COLUMNS[section])
        return None

    def _request_thumb(self, row: int) -> None:
        if row in self._requested:
            return
        self._requested.add(row)
        r = self.rows[row]
        if r.output_path is None:
            return
        gen = self._generation
        path = r.output_path
        Task.submit(
            self.pool,
            lambda: load_reduced(path, THUMB * 2, THUMB)[0],
            lambda img: self._thumb_ready(gen, row, img),
        )

    def _thumb_ready(self, gen: int, row: int, img: QImage) -> None:
        if gen != self._generation or row >= len(self.rows):
            return
        pm = QPixmap.fromImage(img).scaled(
            THUMB, THUMB, Qt.AspectRatioMode.KeepAspectRatio, Qt.TransformationMode.SmoothTransformation
        )
        self.thumbs[row] = pm
        idx = self.index(row, 0)
        self.dataChanged.emit(idx, idx, [Qt.ItemDataRole.DecorationRole])

    def data(
        self, index: QModelIndex | QPersistentModelIndex, role: int = Qt.ItemDataRole.DisplayRole
    ) -> Any:
        if not index.isValid():
            return None
        r = self.rows[index.row()]
        col = index.column()
        if role == RESULT_ROLE:
            return r
        if role == Qt.ItemDataRole.DecorationRole and col == 0:
            if not is_success(r):
                return None
            pm = self.thumbs.get(index.row())
            if pm is None:
                self._request_thumb(index.row())  # only visible rows are asked: lazy loading
                return self._placeholder
            return pm
        if role == Qt.ItemDataRole.DisplayRole:
            if col == 0:
                return '' if is_success(r) else '✗'
            if col == 1:
                return r.output_path.name if r.output_path else r.input_path.name
            if col == 2:
                return r.rel_dir or '—'
            if col == 3:
                return f'{r.width}×{r.height} · {human_size(r.size_bytes)}' if r.width else '—'
            if col == 4:
                return status_text(r)
        if role == SORT_ROLE:
            if col in (0, 1):
                return (r.rel_dir.casefold(), (r.output_path or r.input_path).name.casefold())
            if col == 2:
                return (r.rel_dir.casefold(), r.input_path.name.casefold())
            if col == 3:
                return (r.width or 0) * (r.height or 0)
            if col == 4:
                return ('error', 'warning', 'cancelled', 'ok').index(r.status)
        if role == Qt.ItemDataRole.ForegroundRole:
            if r.status == 'error':
                return QColor('#f87171')
            if col == 4:
                return QColor(STATUS_COLOR[r.status])
        if role == Qt.ItemDataRole.TextAlignmentRole and col == 0:
            return Qt.AlignmentFlag.AlignCenter
        if role == Qt.ItemDataRole.ToolTipRole:
            return f'{r.input_path}\n{tr_msg(r.message)}' if r.message else str(r.output_path or r.input_path)
        return None


class ResultsProxy(QSortFilterProxyModel):
    def __init__(self, parent: Any = None) -> None:
        super().__init__(parent)
        self.mode = 'all'
        self.search = ''
        self.setSortRole(SORT_ROLE)

    def set_mode(self, mode: str) -> None:
        self.mode = mode
        self.invalidateFilter()

    def set_search(self, text: str) -> None:
        self.search = text.casefold().strip()
        self.invalidateFilter()

    def filterAcceptsRow(self, row: int, parent: QModelIndex | QPersistentModelIndex) -> bool:  # noqa: N802
        r: FileResult = cast(ResultsModel, self.sourceModel()).rows[row]
        if self.mode == 'ok' and r.status not in ('ok', 'warning'):
            return False
        if self.mode == 'failed' and r.status != 'error':
            return False
        if self.search:
            name = (r.output_path or r.input_path).name.casefold()
            if self.search not in name and self.search not in r.input_path.name.casefold():
                return False
        return True

    def lessThan(
        self, a: QModelIndex | QPersistentModelIndex, b: QModelIndex | QPersistentModelIndex
    ) -> bool:  # noqa: N802
        return self.sourceModel().data(a, SORT_ROLE) < self.sourceModel().data(b, SORT_ROLE)


class ResultsTable(QTableView):
    enter_pressed = Signal(QModelIndex)

    def keyPressEvent(self, e: QKeyEvent) -> None:  # noqa: N802
        if e.key() in (Qt.Key.Key_Return, Qt.Key.Key_Enter) and self.currentIndex().isValid():
            self.enter_pressed.emit(self.currentIndex())
            return
        super().keyPressEvent(e)


class ResultsView(QWidget):
    open_viewer = Signal(list, int)  # successful FileResults in list order, index to open
    open_in_photo_editor = Signal(object)  # Path (right-click menu)
    new_folder = Signal()
    run_again = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.report: BatchReport | None = None
        self.banner = Banner()
        self.model = ResultsModel(self)
        self.proxy = ResultsProxy(self)
        self.proxy.setSourceModel(self.model)
        self.table = ResultsTable()
        self.table.setModel(self.proxy)
        self.table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        self.table.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        self.table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)
        self.table.setAlternatingRowColors(True)
        self.table.setSortingEnabled(True)
        self.table.setIconSize(QSize(THUMB, THUMB))
        self.table.verticalHeader().setVisible(False)
        self.table.verticalHeader().setSectionResizeMode(QHeaderView.ResizeMode.Fixed)
        self.table.verticalHeader().setDefaultSectionSize(THUMB + 8)
        self.table.setVerticalScrollMode(QAbstractItemView.ScrollMode.ScrollPerPixel)
        self.table.setWordWrap(False)
        self.table.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        hdr = self.table.horizontalHeader()
        hdr.setSectionResizeMode(0, QHeaderView.ResizeMode.Fixed)
        hdr.resizeSection(0, THUMB + 16)
        hdr.setSectionResizeMode(1, QHeaderView.ResizeMode.Stretch)
        hdr.setSectionResizeMode(2, QHeaderView.ResizeMode.ResizeToContents)
        hdr.setSectionResizeMode(3, QHeaderView.ResizeMode.ResizeToContents)
        hdr.setSectionResizeMode(4, QHeaderView.ResizeMode.Interactive)
        hdr.resizeSection(4, 280)

        self.filter_all = QPushButton()
        self.filter_ok = QPushButton()
        self.filter_failed = QPushButton()
        group = QButtonGroup(self)
        for b, mode in ((self.filter_all, 'all'), (self.filter_ok, 'ok'), (self.filter_failed, 'failed')):
            b.setObjectName('Filter')
            b.setCheckable(True)
            group.addButton(b)
            b.clicked.connect(lambda _=False, m=mode: self.proxy.set_mode(m))
        self.filter_all.setChecked(True)
        self.search = QLineEdit()
        self.search.setPlaceholderText(tr('Search by file name'))
        self.search.setClearButtonEnabled(True)
        self.search.setMaximumWidth(280)
        self.search.textChanged.connect(self.proxy.set_search)

        frow = QHBoxLayout()
        lab = QLabel(tr('Filter:'))
        lab.setProperty('secondary', True)
        frow.addWidget(lab)
        frow.addWidget(self.filter_all)
        frow.addWidget(self.filter_ok)
        frow.addWidget(self.filter_failed)
        frow.addStretch(1)
        slab = QLabel(tr('Search:'))
        slab.setProperty('secondary', True)
        frow.addWidget(slab)
        frow.addWidget(self.search)

        self.new_btn = button(tr('New Folder'), 'arrow-left')
        self.again_btn = button(tr('Adjust && Run Again'), 'repeat')
        self.open_btn = button(tr('Open Output Folder'), 'folder', 'Accent')
        brow = QHBoxLayout()
        brow.addWidget(self.new_btn)
        brow.addStretch(1)
        brow.addWidget(self.again_btn)
        brow.addStretch(1)
        brow.addWidget(self.open_btn)

        lay = QVBoxLayout(self)
        lay.setContentsMargins(16, 14, 16, 14)
        lay.setSpacing(10)
        lay.addWidget(self.banner)
        lay.addLayout(frow)
        lay.addWidget(self.table, 1)
        lay.addLayout(brow)

        self.table.clicked.connect(self._activate)
        self.table.enter_pressed.connect(self._activate)
        self.table.customContextMenuRequested.connect(self._context_menu)
        self.banner.open_folder.connect(self.open_output_folder)
        self.open_btn.clicked.connect(self.open_output_folder)
        self.new_btn.clicked.connect(self.new_folder.emit)
        self.again_btn.clicked.connect(self.run_again.emit)

    def set_report(self, report: BatchReport) -> None:
        self.report = report
        self.banner.show_report(report)
        self.model.set_report(report)
        ok = report.succeeded
        failed = report.count('error')
        self.filter_all.setText(tr('All {n}', n=len(report.files)))
        self.filter_ok.setText(tr('Succeeded {n}', n=ok))
        self.filter_failed.setText(tr('Failed {n}', n=failed))
        self.filter_all.setChecked(True)
        self.proxy.set_mode('all')
        self.search.clear()
        # default order: relative path, as in the input (no column sort)
        self.table.horizontalHeader().setSortIndicator(-1, Qt.SortOrder.AscendingOrder)
        self.proxy.sort(-1)
        if self.proxy.rowCount():
            self.table.selectRow(0)
        self.table.setFocus()

    def clear(self) -> None:
        self.report = None
        self.model.set_report(None)

    def visible_successes(self) -> list[FileResult]:
        out = []
        for row in range(self.proxy.rowCount()):
            r = self.proxy.data(self.proxy.index(row, 0), RESULT_ROLE)
            if is_success(r):
                out.append(r)
        return out

    def _activate(self, index: QModelIndex) -> None:
        r: FileResult = self.proxy.data(index.siblingAtColumn(0), RESULT_ROLE)
        if r is None:
            return
        if not is_success(r):
            if r.status == 'error':
                QMessageBox.warning(self, tr('Photo failed'), f'{r.input_path}\n\n{tr_msg(r.message)}')
            return
        items = self.visible_successes()
        self.open_viewer.emit(items, next(i for i, x in enumerate(items) if x is r))

    def select_result(self, r: FileResult) -> None:
        for row in range(self.proxy.rowCount()):
            if self.proxy.data(self.proxy.index(row, 0), RESULT_ROLE) is r:
                self.table.selectRow(row)
                self.table.scrollTo(self.proxy.index(row, 0))
                return

    def _context_menu(self, pos: Any) -> None:
        index = self.table.indexAt(pos)
        if not index.isValid():
            return
        r: FileResult = self.proxy.data(index.siblingAtColumn(0), RESULT_ROLE)
        path = r.output_path if is_success(r) and r.output_path else r.input_path
        menu = QMenu(self)
        a_edit = QAction(tr('Open in Photo editor'), menu)
        a_open = QAction(tr('Open with default app'), menu)
        a_show = QAction(tr('Show in Finder') if IS_MAC else tr('Show in File Explorer'), menu)
        a_copy = QAction(tr('Copy path'), menu)
        for a in (a_edit, a_open, a_show, a_copy):
            menu.addAction(a)
        exists = path is not None and Path(path).exists()
        a_edit.setEnabled(exists)
        a_open.setEnabled(exists)
        chosen = menu.exec(self.table.viewport().mapToGlobal(pos))
        if chosen is a_edit:
            self.open_in_photo_editor.emit(Path(path))
        elif chosen is a_open:
            open_with_default_app(path)
        elif chosen is a_show:
            open_in_file_manager(path, select=True)
        elif chosen is a_copy:
            QGuiApplication.clipboard().setText(str(path))

    def open_output_folder(self) -> None:
        if self.report and Path(self.report.output_dir).exists():
            open_in_file_manager(self.report.output_dir)
