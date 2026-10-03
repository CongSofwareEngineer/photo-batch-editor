"""Single-photo editor page: tools (left), canvas (centre), properties + layers (right),
file / undo bar (top), zoom and size (bottom)."""

from __future__ import annotations

import logging
from pathlib import Path

from PySide6.QtCore import QPointF, QSize, Qt, QThreadPool, Signal
from PySide6.QtGui import QDragEnterEvent, QDropEvent, QImage, QKeySequence, QShortcut
from PySide6.QtWidgets import (
    QApplication,
    QButtonGroup,
    QDialog,
    QDialogButtonBox,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMenu,
    QMessageBox,
    QPlainTextEdit,
    QRadioButton,
    QScrollArea,
    QSplitter,
    QStackedWidget,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from core.i18n import tr, tr_msg
from core.io_utils import open_in_file_manager
from core.photo.image_io import EXPORT_FORMATS, default_export_path, editor_dir, load_rgba, save_image
from ui.icons import icon
from ui.photo.canvas import PhotoCanvas
from ui.photo.collage_dialog import IMAGE_FILTER, CollageDialog
from ui.photo.document import Document, ImageLayer, TextLayer
from ui.photo.panels import (
    AdjustProps,
    BlurProps,
    CropProps,
    ImageProps,
    LayerPanel,
    RotateProps,
    SelectProps,
    TextProps,
    ValueSlider,
)
from ui.photo.project_io import PROJECT_EXT, ProjectError, load_project, save_project
from ui.photo.render import qimage_from_rgba, rgba_from_qimage
from ui.widgets import ElidedLabel, button, tool_button
from ui.workers import Task

log = logging.getLogger(__name__)

# key, label, icon, shortcut, tooltip
TOOLS = (
    ('select', 'Select', 'pointer', 'V', 'Select, move, resize and rotate layers (V)'),
    ('move', 'Move', 'move', 'M', 'Move the selected layer (M)'),
    ('crop', 'Crop', 'crop', 'C', 'Crop the photo (C)'),
    ('text', 'Text', 'type', 'T', 'Add or edit text (T)'),
    ('insert', 'Insert image', 'image-plus', 'I', 'Insert another image as a layer (I)'),
    ('blur', 'Blur', 'droplet', 'B', 'Blur or pixelate an area (B)'),
    ('adjust', 'Adjust', 'sun', 'A', 'Brightness, contrast, saturation, temperature, sharpness (A)'),
    ('rotate', 'Rotate', 'rotate-cw', 'R', 'Rotate and flip (R)'),
    ('collage', 'Collage', 'layout', 'L', 'Make a collage from several photos (L)'),
)
ACTION_TOOLS = ('insert', 'collage')  # buttons that open a dialog instead of staying active
PROJECT_FILTER = 'Photo editor project (*.pbep)'


class TextDialog(QDialog):
    """Multi-line text input (Vietnamese input methods work as in any text box)."""

    def __init__(self, text: str, title: str, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle(title)
        self.resize(460, 220)
        self.edit = QPlainTextEdit(text)
        self.edit.setPlaceholderText(tr('Your text'))
        self.edit.selectAll()
        box = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel)
        box.button(QDialogButtonBox.StandardButton.Ok).setText(tr('OK'))
        box.button(QDialogButtonBox.StandardButton.Cancel).setText(tr('Cancel'))
        box.accepted.connect(self.accept)
        box.rejected.connect(self.reject)
        lay = QVBoxLayout(self)
        lay.addWidget(QLabel(tr('Text (Enter = new line):')))
        lay.addWidget(self.edit, 1)
        lay.addWidget(box)

    def text(self) -> str:
        return self.edit.toPlainText().strip('\n')


class ExportDialog(QDialog):
    def __init__(self, doc: Document, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setWindowTitle(tr('Export'))
        self.setMinimumWidth(560)
        self.doc = doc
        self.jpeg = QRadioButton('JPEG (.jpg)')
        self.png = QRadioButton(tr('PNG (.png, keeps transparency)'))
        self.jpeg.setChecked(True)
        self.quality = ValueSlider(tr('JPEG quality'), 1, 100, 92)
        self.path = QLineEdit()
        browse = button(tr('Browse…'), 'folder')
        row = QHBoxLayout()
        row.addWidget(self.path, 1)
        row.addWidget(browse)
        fmt = QHBoxLayout()
        fmt.addWidget(self.jpeg)
        fmt.addWidget(self.png)
        fmt.addStretch(1)
        box = QDialogButtonBox()
        box.addButton(tr('Export'), QDialogButtonBox.ButtonRole.AcceptRole)
        box.addButton(tr('Cancel'), QDialogButtonBox.ButtonRole.RejectRole)
        box.accepted.connect(self._accept)
        box.rejected.connect(self.reject)
        lay = QVBoxLayout(self)
        lay.addWidget(QLabel(tr('Format')))
        lay.addLayout(fmt)
        lay.addWidget(self.quality)
        lay.addWidget(QLabel(tr('Save as')))
        lay.addLayout(row)
        lay.addWidget(
            QLabel(tr('The original photo is never overwritten. Default folder: "editor" next to the photo.'))
        )
        lay.addWidget(box)
        self.jpeg.toggled.connect(self._format_changed)
        browse.clicked.connect(self._browse)
        self._format_changed()

    def fmt(self) -> str:
        return 'jpeg' if self.jpeg.isChecked() else 'png'

    def _format_changed(self) -> None:
        self.quality.setEnabled(self.fmt() == 'jpeg')
        cur = self.path.text().strip()
        stem = Path(self.doc.name).stem if self.doc.name else 'image'
        if cur:
            self.path.setText(str(Path(cur).with_suffix(EXPORT_FORMATS[self.fmt()])))
        else:
            self.path.setText(str(default_export_path(self.doc.source, self.fmt(), stem)))

    def _browse(self) -> None:
        flt = 'JPEG (*.jpg *.jpeg)' if self.fmt() == 'jpeg' else 'PNG (*.png)'
        path, _ = QFileDialog.getSaveFileName(self, tr('Export'), self.path.text(), flt)
        if path:
            self.path.setText(path)

    def target(self) -> Path:
        return Path(self.path.text().strip())

    def _accept(self) -> None:
        target = self.target()
        if not self.path.text().strip():
            return
        if self.doc.source is not None and target.resolve() == self.doc.source.resolve():
            QMessageBox.warning(
                self,
                tr('Export'),
                tr('This would overwrite the original photo. Choose another name or folder.'),
            )
            return
        if (
            target.exists()
            and QMessageBox.question(
                self, tr('Export'), tr('{name} already exists. Replace it?', name=target.name)
            )
            != QMessageBox.StandardButton.Yes
        ):
            return
        self.accept()


class PhotoEditorView(QWidget):
    status_message = Signal(str)

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Page')
        self.setAcceptDrops(True)
        self.doc: Document | None = None
        self.last_dir = ''
        self.pool = QThreadPool(self)
        self.pool.setMaxThreadCount(1)
        self._busy = False

        # top bar
        self.open_btn = button(tr('Open'), 'folder', 'Accent', tr('Open a photo (Ctrl+O)'))
        self.save_btn = button(
            tr('Save project'), 'save', tooltip=tr('Save the project with its layers (Ctrl+S)')
        )
        self.export_btn = button(tr('Export'), 'download', 'Primary', tr('Export as JPEG or PNG (Ctrl+E)'))
        self.undo_btn = tool_button('undo', tr('Undo (Ctrl+Z)'))
        self.redo_btn = tool_button('redo', tr('Redo (Ctrl+Y)'))
        self.more_btn = QToolButton()
        self.more_btn.setIcon(icon('menu'))
        self.more_btn.setToolTip(tr('More'))
        self.more_btn.setPopupMode(QToolButton.ToolButtonPopupMode.InstantPopup)
        menu = QMenu(self.more_btn)
        menu.addAction(icon('folder'), tr('Open project…'), self.open_project_dialog)
        menu.addAction(icon('save'), tr('Save project as…'), lambda: self.save_project(save_as=True))
        menu.addSeparator()
        menu.addAction(icon('layout'), tr('New collage…'), self.new_collage)
        menu.addSeparator()
        menu.addAction(icon('trash'), tr('Close photo'), self.close_document)
        self.more_btn.setMenu(menu)
        self.title = ElidedLabel(tr('No photo open'))
        self.title.setProperty('secondary', True)
        top_box = QFrame()
        top_box.setObjectName('Panel')
        top = QHBoxLayout(top_box)
        top.setContentsMargins(10, 8, 10, 8)
        top.setSpacing(8)
        top.addWidget(self.open_btn)
        top.addWidget(self.save_btn)
        top.addWidget(self.export_btn)
        top.addSpacing(10)
        top.addWidget(self.undo_btn)
        top.addWidget(self.redo_btn)
        top.addSpacing(10)
        top.addWidget(self.title, 1)
        top.addWidget(self.more_btn)

        # tools
        self.tool_group = QButtonGroup(self)
        self.tool_group.setExclusive(True)
        self.tool_buttons: dict[str, QToolButton] = {}
        tools_box = QFrame()
        tools_box.setObjectName('Panel')
        tl = QVBoxLayout(tools_box)
        tl.setContentsMargins(6, 8, 6, 8)
        tl.setSpacing(4)
        for key, label, icon_name, _keys, tip in TOOLS:
            b = QToolButton()
            b.setObjectName('ToolButton')
            b.setIcon(icon(icon_name))
            b.setIconSize(QSize(20, 20))
            b.setToolTip(tr(tip))
            b.setAccessibleName(tr(label))
            b.setFixedSize(40, 40)
            b.setCursor(Qt.CursorShape.PointingHandCursor)
            if key not in ACTION_TOOLS:
                b.setCheckable(True)
                self.tool_group.addButton(b)
            b.clicked.connect(lambda _=False, k=key: self.activate_tool(k))
            self.tool_buttons[key] = b
            tl.addWidget(b)
        tl.addStretch(1)

        # canvas
        self.canvas = PhotoCanvas()

        # right: properties + layers
        self.text_props = TextProps()
        self.image_props = ImageProps()
        self.blur_props = BlurProps()
        self.crop_props = CropProps()
        self.adjust_props = AdjustProps()
        self.rotate_props = RotateProps()
        self.select_props = SelectProps(self.text_props, self.image_props, self.blur_props)
        self.props_stack = QStackedWidget()
        for w in (
            self.select_props,
            self.text_props,
            self.image_props,
            self.blur_props,
            self.crop_props,
            self.adjust_props,
            self.rotate_props,
        ):
            self.props_stack.addWidget(w)
        props_box = QFrame()
        props_box.setObjectName('Panel')
        pl = QVBoxLayout(props_box)
        pl.setContentsMargins(12, 10, 12, 10)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.Shape.NoFrame)
        scroll.setWidget(self.props_stack)
        pl.addWidget(scroll)
        self.layer_panel = LayerPanel()
        right = QSplitter(Qt.Orientation.Vertical)
        right.addWidget(props_box)
        right.addWidget(self.layer_panel)
        right.setSizes([440, 300])
        right.setChildrenCollapsible(False)
        right.setFixedWidth(330)

        middle = QHBoxLayout()
        middle.setSpacing(10)
        middle.addWidget(tools_box)
        middle.addWidget(self.canvas, 1)
        middle.addWidget(right)

        # status bar
        self.zoom_out_btn = tool_button('chevron-down', tr('Zoom out (Ctrl+-)'))
        self.zoom_label = QLabel('—')
        self.zoom_label.setMinimumWidth(56)
        self.zoom_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.zoom_in_btn = tool_button('chevron-up', tr('Zoom in (Ctrl++)'))
        self.fit_btn = button(tr('Fit'), tooltip=tr('Fit on screen (Ctrl+0)'))
        self.actual_btn = button('100%', tooltip=tr('Actual pixels (Ctrl+1)'))
        self.size_label = QLabel('')
        self.size_label.setProperty('secondary', True)
        self.pos_label = QLabel('')
        self.pos_label.setProperty('secondary', True)
        self.pos_label.setMinimumWidth(110)
        self.message = ElidedLabel('')
        self.message.setProperty('secondary', True)
        self.show_btn = button(tr('Show in folder'), 'folder', 'Ghost')
        self.show_btn.hide()
        self._last_export: Path | None = None
        status_box = QFrame()
        status_box.setObjectName('Panel')
        sl = QHBoxLayout(status_box)
        sl.setContentsMargins(10, 4, 10, 4)
        sl.setSpacing(6)
        for w in (self.zoom_out_btn, self.zoom_label, self.zoom_in_btn, self.fit_btn, self.actual_btn):
            sl.addWidget(w)
        sl.addSpacing(12)
        sl.addWidget(self.size_label)
        sl.addSpacing(12)
        sl.addWidget(self.pos_label)
        sl.addWidget(self.message, 1)
        sl.addWidget(self.show_btn)

        lay = QVBoxLayout(self)
        lay.setContentsMargins(16, 14, 16, 14)
        lay.setSpacing(10)
        lay.addWidget(top_box)
        lay.addLayout(middle, 1)
        lay.addWidget(status_box)

        # wiring
        self.open_btn.clicked.connect(self.open_dialog)
        self.save_btn.clicked.connect(lambda: self.save_project())
        self.export_btn.clicked.connect(self.export_dialog)
        self.undo_btn.clicked.connect(self.undo)
        self.redo_btn.clicked.connect(self.redo)
        self.zoom_out_btn.clicked.connect(lambda: self.canvas.zoom_step(-1))
        self.zoom_in_btn.clicked.connect(lambda: self.canvas.zoom_step(1))
        self.fit_btn.clicked.connect(self.canvas.fit)
        self.actual_btn.clicked.connect(self.canvas.actual_size)
        self.show_btn.clicked.connect(
            lambda: self._last_export and open_in_file_manager(self._last_export, select=True)
        )
        self.canvas.zoom_changed.connect(lambda s: self.zoom_label.setText(f'{s * 100:.0f}%'))
        self.canvas.cursor_moved.connect(self._cursor_moved)
        self.canvas.create_text_requested.connect(self.create_text)
        self.canvas.edit_text_requested.connect(self.edit_text)
        self.canvas.tool_state_changed.connect(self._tool_state_changed)
        self.blur_props.options_changed.connect(self._blur_options)
        self.crop_props.ratio_changed.connect(lambda r: self.canvas.tools['crop'].set_ratio(r))  # type: ignore
        self.crop_props.apply_requested.connect(lambda: self.canvas.tools['crop'].apply())  # type: ignore
        self.crop_props.cancel_requested.connect(lambda: self.canvas.tools['crop'].cancel())  # type: ignore
        self.rotate_props.after_canvas_change = self.canvas.fit

        shortcuts = (
            ('Ctrl+O', self.open_dialog),
            ('Ctrl+S', lambda: self.save_project()),
            ('Ctrl+Shift+S', lambda: self.save_project(save_as=True)),
            ('Ctrl+E', self.export_dialog),
            ('Ctrl+Z', self.undo),
            ('Ctrl+Y', self.redo),
            ('Ctrl+Shift+Z', self.redo),
            ('Ctrl+0', self.canvas.fit),
            ('Ctrl+1', self.canvas.actual_size),
            ('Ctrl++', lambda: self.canvas.zoom_step(1)),
            ('Ctrl+=', lambda: self.canvas.zoom_step(1)),
            ('Ctrl+-', lambda: self.canvas.zoom_step(-1)),
            ('Escape', self._escape),
        )
        for keys, slot in shortcuts:
            sc = QShortcut(QKeySequence(keys), self)
            sc.setContext(Qt.ShortcutContext.WidgetWithChildrenShortcut)
            sc.activated.connect(slot)
        for key, _label, _icon, keys, _tip in TOOLS:
            sc = QShortcut(QKeySequence(keys), self)
            sc.setContext(Qt.ShortcutContext.WidgetWithChildrenShortcut)
            sc.activated.connect(lambda k=key: self.activate_tool(k))

        self.tool_buttons['select'].setChecked(True)
        self.current_tool = 'select'
        self._blur_options()
        self._set_document(None)

    # document --------------------------------------------------------------------------------
    def _set_document(self, doc: Document | None) -> None:
        if self.doc is not None and self._doc_event in self.doc.listeners:
            self.doc.listeners.remove(self._doc_event)
        self.doc = doc
        if doc is not None:
            doc.listeners.append(self._doc_event)
            w, h = doc.size
            self.text_props.defaults.font_size = float(max(12, round(min(w, h) / 12)))
            self.canvas.tools['blur'].brush_size = float(max(8, round(max(w, h) / 25)))  # type: ignore
            self.blur_props.set_brush_size(self.canvas.tools['blur'].brush_size)  # type: ignore
        self.canvas.set_document(doc)
        for panel in (
            self.text_props,
            self.image_props,
            self.blur_props,
            self.crop_props,
            self.adjust_props,
            self.rotate_props,
            self.layer_panel,
        ):
            panel.set_document(doc)
        has = doc is not None
        for w in (
            self.save_btn,
            self.export_btn,
            *self.tool_buttons.values(),
            self.fit_btn,
            self.actual_btn,
            self.zoom_in_btn,
            self.zoom_out_btn,
        ):
            w.setEnabled(has)
        self.tool_buttons['collage'].setEnabled(True)
        if not has:
            self.zoom_label.setText('—')
        self._update_title()
        self._update_history()
        self._update_props_page()
        self._tool_state_changed()

    def take_document(self) -> Document | None:
        """Hand the document to a rebuilt window (language switch)."""
        doc = self.doc
        if doc is not None and self._doc_event in doc.listeners:
            doc.listeners.remove(self._doc_event)
        self.doc = None
        self.canvas.set_document(None)
        return doc

    def adopt_document(self, doc: Document | None, last_dir: str = '') -> None:
        self.last_dir = last_dir or self.last_dir
        if doc is not None:
            doc.listeners.clear()
            self._set_document(doc)

    def _doc_event(self, what: str) -> None:
        if what == 'layers':
            self.layer_panel.refresh()
            self._refresh_props()
        elif what == 'selection':
            self.layer_panel.sync_selection()
            self._update_props_page()
            self._refresh_props()
        elif what == 'content':
            self._refresh_props()
        self._update_history()
        self._update_title()
        if what in ('content', 'layers') and self.doc is not None:
            self.size_label.setText(f'{self.doc.width} × {self.doc.height} px')

    def _refresh_props(self) -> None:
        w = self.props_stack.currentWidget()
        refresh = getattr(w, 'refresh', None)
        if callable(refresh):
            refresh()
        self.layer_panel._update_buttons()

    def _update_history(self) -> None:
        h = self.doc.history if self.doc else None
        self.undo_btn.setEnabled(bool(h and h.can_undo()))
        self.redo_btn.setEnabled(bool(h and h.can_redo()))
        self.undo_btn.setToolTip(
            tr('Undo (Ctrl+Z)') + (f' — {tr(h.undo_label())}' if h and h.can_undo() else '')
        )
        self.redo_btn.setToolTip(
            tr('Redo (Ctrl+Y)') + (f' — {tr(h.redo_label())}' if h and h.can_redo() else '')
        )

    def _update_title(self) -> None:
        if self.doc is None:
            self.title.setText(tr('No photo open — open a photo, drop one here, or make a collage'))
            self.size_label.setText('')
            return
        name = self.doc.project_path.name if self.doc.project_path else (self.doc.name or tr('Untitled'))
        self.title.setText(('● ' if self.doc.modified else '') + name)
        self.size_label.setText(f'{self.doc.width} × {self.doc.height} px')

    def is_modified(self) -> bool:
        return self.doc is not None and self.doc.modified

    def confirm_discard(self) -> bool:
        """``True`` when the current document may be replaced / closed."""
        if not self.is_modified():
            return True
        box = QMessageBox(self)
        box.setIcon(QMessageBox.Icon.Question)
        box.setWindowTitle(tr('Photo editor'))
        box.setText(tr('The photo has changes that are not saved or exported.'))
        box.setInformativeText(tr('Save them as a project first?'))
        save = box.addButton(tr('Save project'), QMessageBox.ButtonRole.AcceptRole)
        discard = box.addButton(tr('Discard'), QMessageBox.ButtonRole.DestructiveRole)
        box.addButton(tr('Cancel'), QMessageBox.ButtonRole.RejectRole)
        box.setDefaultButton(save)
        box.exec()
        if box.clickedButton() is save:
            return self.save_project()
        return box.clickedButton() is discard

    # open / save ------------------------------------------------------------------------------
    def open_dialog(self) -> None:
        if not self.confirm_discard():
            return
        flt = f'{tr(IMAGE_FILTER)};;{tr(PROJECT_FILTER)}'
        path, _ = QFileDialog.getOpenFileName(self, tr('Open a photo'), self.last_dir, flt)
        if path:
            self.open_path(Path(path), confirm=False)

    def open_project_dialog(self) -> None:
        if not self.confirm_discard():
            return
        path, _ = QFileDialog.getOpenFileName(self, tr('Open project'), self.last_dir, tr(PROJECT_FILTER))
        if path:
            self.open_path(Path(path), confirm=False)

    def open_path(self, path: Path, confirm: bool = True) -> None:
        """Open a photo or a ``.pbep`` project (from the batch editor, a drop, the dialog)."""
        if confirm and not self.confirm_discard():
            return
        self.last_dir = str(path.parent)
        if path.suffix.lower() == PROJECT_EXT:
            QApplication.setOverrideCursor(Qt.CursorShape.WaitCursor)
            try:
                doc = load_project(path)
            except ProjectError as exc:
                QMessageBox.warning(self, tr('Open project'), tr_msg(str(exc)))
                return
            finally:
                QApplication.restoreOverrideCursor()
            self._set_document(doc)
            self.show_message(tr('Project opened: {name}', name=path.name))
            return
        self._set_busy(True, tr('Opening {name}…', name=path.name))
        Task.submit(
            self.pool,
            lambda: qimage_from_rgba(load_rgba(path)),
            lambda img: self._opened(path, img),
            lambda err: self._open_failed(path, err),
        )

    def _opened(self, path: Path, img: QImage) -> None:
        self._set_busy(False)
        self._set_document(Document(img, path))
        self.show_message(tr('Opened {name}', name=path.name))

    def _open_failed(self, path: Path, err: str) -> None:
        self._set_busy(False)
        QMessageBox.warning(
            self, tr('Open a photo'), tr('Cannot open the photo:\n{path}\n\n{error}', path=path, error=err)
        )

    def open_image(self, img: QImage, name: str) -> None:
        """A new document from an image made in the app (collage)."""
        self._set_document(Document(img, None, name))
        self.doc.modified = True
        self._update_title()

    def close_document(self) -> None:
        if self.confirm_discard():
            self._set_document(None)

    def save_project(self, save_as: bool = False) -> bool:
        if self.doc is None:
            return False
        path = self.doc.project_path
        if path is None or save_as:
            base = self.doc.source or Path(self.last_dir or editor_dir(None)) / (self.doc.name or 'project')
            default = (editor_dir(self.doc.source) / Path(base).name).with_suffix(PROJECT_EXT)
            chosen, _ = QFileDialog.getSaveFileName(
                self, tr('Save project'), str(default), tr(PROJECT_FILTER)
            )
            if not chosen:
                return False
            path = Path(chosen)
            if path.suffix.lower() != PROJECT_EXT:
                path = path.with_suffix(PROJECT_EXT)
        QApplication.setOverrideCursor(Qt.CursorShape.WaitCursor)
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            save_project(self.doc, path)
        except OSError as exc:
            QMessageBox.warning(
                self, tr('Save project'), tr('Could not save the project: {error}', error=exc)
            )
            return False
        finally:
            QApplication.restoreOverrideCursor()
        self._update_title()
        self.show_message(tr('Project saved: {path}', path=path))
        return True

    def export_dialog(self) -> None:
        if self.doc is None or self._busy:
            return
        dlg = ExportDialog(self.doc, self)
        if dlg.exec() != QDialog.DialogCode.Accepted:
            return
        target, fmt, quality = dlg.target(), dlg.fmt(), int(dlg.quality.value())
        QApplication.setOverrideCursor(Qt.CursorShape.WaitCursor)
        try:
            rgba = rgba_from_qimage(self.canvas.renderer.render_image(self.doc.state))
        finally:
            QApplication.restoreOverrideCursor()
        self._set_busy(True, tr('Exporting…'))
        Task.submit(
            self.pool,
            lambda: save_image(target, rgba, fmt, quality),
            lambda p: self._exported(p),
            lambda err: self._export_failed(err),
        )

    def _exported(self, path: Path) -> None:
        self._set_busy(False)
        self._last_export = path
        if self.doc is not None and self.doc.project_path is None:
            self.doc.mark_saved()
        self._update_title()
        self.show_message(tr('Exported: {path}', path=path))
        self.show_btn.show()

    def _export_failed(self, err: str) -> None:
        self._set_busy(False)
        QMessageBox.warning(self, tr('Export'), tr('Could not export the image: {error}', error=err))

    def _set_busy(self, busy: bool, message: str = '') -> None:
        self._busy = busy
        if busy:
            QApplication.setOverrideCursor(Qt.CursorShape.BusyCursor)
            self.show_message(message)
        else:
            QApplication.restoreOverrideCursor()
        self.export_btn.setEnabled(not busy and self.doc is not None)
        self.open_btn.setEnabled(not busy)

    def show_message(self, text: str) -> None:
        self.message.setText(text)
        self.show_btn.hide()
        self.status_message.emit(text)

    # tools -------------------------------------------------------------------------------------
    def activate_tool(self, key: str) -> None:
        if key == 'collage':
            self.new_collage()
            return
        if self.doc is None:
            return
        if key == 'insert':
            self.insert_image_dialog()
            return
        self.current_tool = key
        self.tool_buttons[key].setChecked(True)
        self.canvas.set_tool(key if key in self.canvas.tools else 'select')
        self._update_props_page()
        self._refresh_props()
        self.canvas.setFocus()

    def _update_props_page(self) -> None:
        key = self.current_tool
        sel = self.doc.selected() if self.doc else None
        page = {
            'text': self.text_props,
            'crop': self.crop_props,
            'blur': self.blur_props,
            'adjust': self.adjust_props,
            'rotate': self.rotate_props,
        }.get(key)
        if page is None:  # select / move: properties of the selected layer
            page = {'text': self.text_props, 'image': self.image_props, 'blur': self.blur_props}.get(
                sel.kind if sel else '', self.select_props
            )
        self.props_stack.setCurrentWidget(page)

    def _escape(self) -> None:
        crop = self.canvas.tools['crop']
        if self.canvas.tool is crop and crop.rect is not None:  # type: ignore[attr-defined]
            crop.cancel()  # type: ignore[attr-defined]
        elif self.doc is not None:
            self.doc.select(None)

    def _blur_options(self) -> None:
        t = self.canvas.tools['blur']
        t.shape = self.blur_props.shape()  # type: ignore[attr-defined]
        t.mode = self.blur_props.mode()  # type: ignore[attr-defined]
        t.strength = self.blur_props.strength.value()  # type: ignore[attr-defined]
        t.brush_size = self.blur_props.brush.value()  # type: ignore[attr-defined]
        self.canvas.update()

    def _tool_state_changed(self) -> None:
        crop = self.canvas.tools['crop']
        r = crop.rect  # type: ignore[attr-defined]
        self.crop_props.set_rect((round(r.width()), round(r.height())) if r is not None else None)
        self.blur_props.set_brush_size(self.canvas.tools['blur'].brush_size)  # type: ignore[attr-defined]

    def _cursor_moved(self, pt: QPointF | None) -> None:
        if (
            pt is None
            or self.doc is None
            or not (0 <= pt.x() < self.doc.width and 0 <= pt.y() < self.doc.height)
        ):
            self.pos_label.setText('')
        else:
            self.pos_label.setText(f'X {int(pt.x())}  Y {int(pt.y())}')

    def create_text(self, x: float, y: float) -> None:
        if self.doc is None:
            return
        dlg = TextDialog('', tr('Add text'), self)
        if dlg.exec() != QDialog.DialogCode.Accepted or not dlg.text().strip():
            return
        self.doc.add_layer(self.text_props.new_layer(x, y, dlg.text()), 'Add text')

    def edit_text(self, layer_id: int) -> None:
        lay = self.doc.layer(layer_id) if self.doc else None
        if not isinstance(lay, TextLayer):
            return
        dlg = TextDialog(lay.text, tr('Edit text'), self)
        if dlg.exec() == QDialog.DialogCode.Accepted and dlg.text().strip():
            text = dlg.text()
            self.doc.edit_layer(layer_id, 'Edit text', text=text, name=text.strip().splitlines()[0][:32])

    def insert_image_dialog(self) -> None:
        path, _ = QFileDialog.getOpenFileName(self, tr('Insert image'), self.last_dir, tr(IMAGE_FILTER))
        if path:
            self.insert_image(Path(path))

    def insert_image(self, path: Path) -> None:
        if self.doc is None:
            return
        try:
            img = qimage_from_rgba(load_rgba(path))
        except Exception as exc:  # noqa: BLE001
            QMessageBox.warning(
                self,
                tr('Insert image'),
                tr('Cannot open the photo:\n{path}\n\n{error}', path=path, error=exc),
            )
            return
        self.last_dir = str(path.parent)
        w, h = self.doc.size
        k = min(1.0, w * 0.5 / img.width(), h * 0.5 / img.height())
        lay = ImageLayer(
            name=path.stem[:32], image=img, x=w / 2, y=h / 2, width=img.width() * k, height=img.height() * k
        )
        self.doc.add_layer(lay, 'Insert image')
        self.activate_tool('select')

    def new_collage(self) -> None:
        dlg = CollageDialog(self, self.last_dir)
        if dlg.exec() != QDialog.DialogCode.Accepted or dlg.result_image is None:
            return
        self.last_dir = dlg.start_dir or self.last_dir
        if not self.confirm_discard():
            return
        self.open_image(dlg.result_image, tr('collage'))
        self.activate_tool('select')

    def undo(self) -> None:
        if self.doc is not None and self.doc.undo():
            self.show_message(tr('Undo'))

    def redo(self) -> None:
        if self.doc is not None and self.doc.redo():
            self.show_message(tr('Redo'))

    # drag & drop ---------------------------------------------------------------------------------
    def dragEnterEvent(self, e: QDragEnterEvent) -> None:  # noqa: N802
        if e.mimeData().hasUrls():
            e.acceptProposedAction()

    def dropEvent(self, e: QDropEvent) -> None:  # noqa: N802
        paths = [Path(u.toLocalFile()) for u in e.mimeData().urls() if u.isLocalFile()]
        if not paths:
            return
        path = paths[0]
        if self.doc is not None and path.suffix.lower() != PROJECT_EXT:
            self.insert_image(path)  # Photoshop-like: dropping on an open photo places it as a layer
        else:
            self.open_path(path)
        e.acceptProposedAction()
