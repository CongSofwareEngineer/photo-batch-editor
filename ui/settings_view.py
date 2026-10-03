"""Settings screen: saved filter settings (create / edit / auto-save) and the account.

A "setting" is a user preset (``core.presets``) stored as JSON in
``%APPDATA%\\PhotoBatchEditor\\presets``; every edit here is written to disk automatically.
Built-in presets are shown as read-only templates.
"""

from __future__ import annotations

import logging
import time
from collections.abc import Callable
from pathlib import Path
from typing import cast

from PySide6.QtCore import QSize, Qt, QTimer, Signal
from PySide6.QtGui import QAction
from PySide6.QtWidgets import (
    QButtonGroup,
    QCheckBox,
    QComboBox,
    QFileDialog,
    QFormLayout,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMenu,
    QMessageBox,
    QScrollArea,
    QSplitter,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from core.auth import AuthError, AuthStore
from core.i18n import LANGUAGES, language, tr, tr_msg
from core.io_utils import open_in_file_manager
from core.presets import (
    Preset,
    delete_user_preset,
    describe_settings,
    display_name,
    list_builtin_presets,
    list_user_presets,
    rename_user_preset,
    save_user_preset,
    unique_name,
)
from core.settings import AdjustmentSettings, load_settings_json, save_settings_json
from ui.adjustment_panel import AdjustmentPanel
from ui.icons import icon
from ui.preview import PreviewPane
from ui.widgets import ElidedLabel, PageHeader, badge, button, set_badge

log = logging.getLogger(__name__)

AUTOSAVE_MS = 400
PRESET_ROLE = Qt.ItemDataRole.UserRole + 1
IMAGE_FILTER = '(*.jpg *.jpeg *.png *.tif *.tiff *.webp *.bmp)'


def summary_text(s: AdjustmentSettings, limit: int = 3) -> str:
    parts = describe_settings(s)
    if not parts:
        return tr('No adjustments')
    more = len(parts) - limit
    return ' · '.join(parts[:limit]) + ('  ' + tr('+{n} more', n=more) if more > 0 else '')


class SettingCard(QFrame):
    """One row of the settings list: name, summary and a badge."""

    def __init__(self, preset: Preset, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('SettingCard')
        self.setAttribute(Qt.WidgetAttribute.WA_Hover, True)
        self.setProperty('selected', False)
        self.name = QLabel()
        self.name.setObjectName('CardName')
        self.summary = ElidedLabel(mode=Qt.TextElideMode.ElideRight)
        self.summary.setObjectName('CardSummary')
        self.icon = QLabel()
        self.icon.setFixedSize(30, 30)
        self.icon.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.icon.setObjectName('CardIcon')
        col = QVBoxLayout()
        col.setSpacing(1)
        col.addWidget(self.name)
        col.addWidget(self.summary)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(10, 8, 10, 8)
        lay.setSpacing(10)
        lay.addWidget(self.icon)
        lay.addLayout(col, 1)
        self.set_preset(preset)

    def set_preset(self, preset: Preset) -> None:
        self.preset = preset
        self.name.setText(display_name(preset))
        text = summary_text(preset.settings, 2)
        self.summary.setText(text)
        self.summary.setToolTip(' · '.join(describe_settings(preset.settings)) or tr('No adjustments'))
        self.icon.setPixmap(
            icon(
                'sparkles' if preset.builtin else 'sliders', '#22d3ee' if preset.builtin else '#6aa8ff'
            ).pixmap(QSize(16, 16))
        )

    def set_selected(self, on: bool) -> None:
        if self.property('selected') != on:
            self.setProperty('selected', on)
            self.summary.setProperty('selected', on)
            for w in (self, self.name, self.summary):
                w.style().unpolish(w)
                w.style().polish(w)


class FilterSettingsPage(QWidget):
    """List of settings on the left, the selected one (preview + adjustments) on the right."""

    apply_requested = Signal(object)  # Preset
    presets_changed = Signal()
    preset_renamed = Signal(str, str)  # old, new

    def __init__(
        self,
        user_preset_dir: Path,
        editor_settings: Callable[[], AdjustmentSettings],
        editor_photo: Callable[[], Path | None],
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.user_preset_dir = Path(user_preset_dir)
        self._editor_settings = editor_settings
        self._editor_photo = editor_photo
        self.presets: list[Preset] = []
        self.current: Preset | None = None
        self.sample_path: Path | None = None  # chosen explicitly; else the Editor's photo
        self._loading = False
        self._dirty = False
        self._locked = False

        # Left: list ------------------------------------------------------------------------
        left = QFrame()
        left.setObjectName('Card')
        left.setFixedWidth(290)
        self.new_btn = button(tr('New'), 'plus', 'Accent', tr('Create a new setting'))
        self.new_menu = QMenu(self)
        self.new_btn.setMenu(self.new_menu)
        self.search = QLineEdit()
        self.search.setPlaceholderText(tr('Search settings'))
        self.search.setClearButtonEnabled(True)
        self.search.addAction(icon('search', '#6f80a3'), QLineEdit.ActionPosition.LeadingPosition)
        self.list = QListWidget()
        self.list.setObjectName('SettingsList')
        self.list.setSpacing(3)
        self.list.setVerticalScrollMode(QListWidget.ScrollMode.ScrollPerPixel)
        self.list.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.folder_btn = button(
            tr('Open settings folder'),
            'folder',
            'Ghost',
            tr('Settings are saved on this computer, in this folder'),
        )
        ll = QVBoxLayout(left)
        ll.setContentsMargins(14, 14, 14, 10)
        ll.setSpacing(10)
        head = QHBoxLayout()
        cap = QLabel(tr('SAVED SETTINGS'))
        cap.setObjectName('SectionTitle')
        self.count_badge = badge('0', 'accent')
        head.addWidget(cap)
        head.addWidget(self.count_badge)
        head.addStretch(1)
        head.addWidget(self.new_btn)
        ll.addLayout(head)
        ll.addWidget(self.search)
        ll.addWidget(self.list, 1)
        ll.addWidget(self.folder_btn)

        # Right: detail ---------------------------------------------------------------------
        right = QFrame()
        right.setObjectName('Card')
        self.name_edit = QLineEdit()
        self.name_edit.setObjectName('TitleEdit')
        self.name_edit.setPlaceholderText(tr('Setting name'))
        self.name_edit.setToolTip(tr('Click to rename'))
        self.kind_badge = badge(tr('Custom'), 'accent')
        self.save_status = QLabel()
        self.save_status.setObjectName('PageSubtitle')
        self.apply_btn = button(
            tr('Use in Editor'), 'check', 'Accent', tr('Apply this setting in the Editor and switch to it')
        )
        self.dup_btn = button(tr('Duplicate'), 'copy', tooltip=tr('Make an editable copy'))
        self.export_btn = button('', 'download', tooltip=tr('Export to a .json file'))
        self.delete_btn = button('', 'trash', 'Danger', tr('Delete this setting'))
        self.summary = QLabel()
        self.summary.setObjectName('PageSubtitle')
        self.summary.setWordWrap(True)
        self.template_note = QLabel(
            tr('Built-in template — read-only. Duplicate it to create your own setting you can edit.')
        )
        self.template_note.setObjectName('Notice')
        self.template_note.setWordWrap(True)

        self.preview = PreviewPane()
        self.sample_label = QLabel()
        self.sample_label.setObjectName('PageSubtitle')
        self.sample_btn = button(
            tr('Sample photo…'), 'image', tooltip=tr('Choose the photo used for this preview')
        )
        self.sample_reset_btn = button('', 'reset', 'Ghost', tr('Use the photo selected in the Editor'))
        bar = QHBoxLayout()
        bar.setContentsMargins(6, 0, 6, 4)
        bar.addWidget(self.sample_label, 1)
        bar.addWidget(self.sample_reset_btn)
        bar.addWidget(self.sample_btn)
        cast(QVBoxLayout, self.preview.layout()).addLayout(bar)
        self.panel = AdjustmentPanel(show_save=False)
        self.panel.setMinimumWidth(360)
        self.preview.setMinimumWidth(320)
        self.body = QSplitter(Qt.Orientation.Horizontal)
        self.body.addWidget(self.preview)
        self.body.addWidget(self.panel)
        self.body.setStretchFactor(0, 1)
        self.body.setStretchFactor(1, 0)
        self.body.setSizes([520, 380])
        self.body.setChildrenCollapsible(False)

        self.empty_detail = QLabel(tr('Select a setting on the left, or create a new one.'))
        self.empty_detail.setObjectName('PageSubtitle')
        self.empty_detail.setAlignment(Qt.AlignmentFlag.AlignCenter)

        rl = QVBoxLayout(right)
        rl.setContentsMargins(16, 12, 16, 14)
        rl.setSpacing(8)
        top = QHBoxLayout()
        top.setSpacing(8)
        top.addWidget(self.name_edit, 1)
        top.addWidget(self.kind_badge)
        top.addSpacing(6)
        top.addWidget(self.save_status)
        actions = QHBoxLayout()
        actions.setSpacing(8)
        actions.addWidget(self.summary, 1)
        actions.addSpacing(10)
        actions.addWidget(self.dup_btn)
        actions.addWidget(self.export_btn)
        actions.addWidget(self.delete_btn)
        actions.addWidget(self.apply_btn)
        self.detail = QWidget()
        self.detail.setObjectName('Transparent')
        dl = QVBoxLayout(self.detail)
        dl.setContentsMargins(0, 0, 0, 0)
        dl.setSpacing(8)
        dl.addLayout(top)
        dl.addLayout(actions)
        dl.addWidget(self.template_note)
        dl.addWidget(self.body, 1)
        rl.addWidget(self.detail, 1)
        rl.addWidget(self.empty_detail, 1)

        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(12)
        lay.addWidget(left)
        lay.addWidget(right, 1)

        # Wiring ----------------------------------------------------------------------------
        self._save_timer = QTimer(self)
        self._save_timer.setSingleShot(True)
        self._save_timer.setInterval(AUTOSAVE_MS)
        self._save_timer.timeout.connect(self.flush)
        self.list.currentItemChanged.connect(self._item_changed)
        self.search.textChanged.connect(self._filter)
        self.panel.settings_changed.connect(self._panel_changed)
        self.name_edit.editingFinished.connect(self._rename)
        self.apply_btn.clicked.connect(self._apply)
        self.dup_btn.clicked.connect(self.duplicate_current)
        self.export_btn.clicked.connect(self._export)
        self.delete_btn.clicked.connect(self.delete_current)
        self.sample_btn.clicked.connect(self._choose_sample)
        self.sample_reset_btn.clicked.connect(lambda: self.set_sample(None))
        self.folder_btn.clicked.connect(self._open_folder)
        self.reload()

    # Data --------------------------------------------------------------------------------------
    def reload(self, select: Path | str | None = None) -> None:
        """Re-read all settings from disk. ``select``: a user preset path or a name."""
        key = select if select is not None else self._key(self.current)
        builtin = list_builtin_presets()
        user = list_user_presets(self.user_preset_dir)
        self.presets = user + builtin
        self._build_new_menu(builtin)
        self.list.blockSignals(True)
        self.list.clear()
        self._add_header(tr('MY SETTINGS'))
        if not user:
            self._add_header(tr('No saved settings yet.\nClick “New” to create your first one.'), empty=True)
        for p in user:
            self._add_item(p)
        self._add_header(tr('TEMPLATES'))
        for p in builtin:
            self._add_item(p)
        self.list.blockSignals(False)
        self.count_badge.setText(str(len(user)))
        self._filter(self.search.text())
        target = self._find(key) or (user[0] if user else (builtin[0] if builtin else None))
        self._select(target)

    @staticmethod
    def _key(p: Preset | None) -> Path | str | None:
        if p is None:
            return None
        return p.path if not p.builtin and p.path else p.name

    def _find(self, key: Path | str | None) -> Preset | None:
        if key is None:
            return None
        for p in self.presets:
            if isinstance(key, Path) and not p.builtin and p.path == key:
                return p
        for p in self.presets:
            if isinstance(key, str) and p.name == key:
                return p
        return None

    def _add_header(self, text: str, empty: bool = False) -> None:
        item = QListWidgetItem()
        item.setFlags(Qt.ItemFlag.NoItemFlags)
        lab = QLabel(text)
        lab.setObjectName('PageSubtitle' if empty else 'SectionTitle')
        if empty:
            lab.setAlignment(Qt.AlignmentFlag.AlignCenter)
            lab.setWordWrap(True)
        item.setSizeHint(QSize(10, 64 if empty else 26))
        self.list.addItem(item)
        self.list.setItemWidget(item, lab)

    def _add_item(self, p: Preset) -> None:
        item = QListWidgetItem()
        item.setData(PRESET_ROLE, p)
        card = SettingCard(p)
        item.setSizeHint(QSize(10, card.sizeHint().height()))
        self.list.addItem(item)
        self.list.setItemWidget(item, card)

    def _items(self) -> list[tuple[QListWidgetItem, SettingCard]]:
        out = []
        for i in range(self.list.count()):
            it = self.list.item(i)
            w = self.list.itemWidget(it)
            if isinstance(w, SettingCard):
                out.append((it, w))
        return out

    def _filter(self, text: str) -> None:
        q = text.strip().casefold()
        for it, card in self._items():
            p = card.preset
            hay = (p.name + ' ' + display_name(p) + ' ' + ' '.join(describe_settings(p.settings))).casefold()
            it.setHidden(bool(q) and q not in hay)

    def _build_new_menu(self, builtin: list[Preset]) -> None:
        m = self.new_menu
        m.clear()
        a = m.addAction(icon('plus'), tr('Blank setting'))
        a.triggered.connect(lambda: self.create(AdjustmentSettings(), tr('New Setting')))
        a = m.addAction(icon('wand'), tr("From the Editor's current adjustments"))
        a.triggered.connect(lambda: self.create(self._editor_settings(), tr('My Setting')))
        sub = m.addMenu(icon('sparkles'), tr('From template'))
        for p in builtin:
            act = QAction(display_name(p), sub)
            act.triggered.connect(
                lambda _=False, p=p: self.create(p.settings, tr('{name} (custom)', name=display_name(p)))
            )
            sub.addAction(act)
        m.addSeparator()
        a = m.addAction(icon('upload'), tr('Import from file…'))
        a.triggered.connect(self._import)

    # Selection --------------------------------------------------------------------------------
    def _select(self, p: Preset | None) -> None:
        for it, card in self._items():
            if card.preset is p:
                self.list.setCurrentItem(it)
                self.list.scrollToItem(it)
                break
        self._show(p)

    def _item_changed(self, cur: QListWidgetItem | None, _prev: QListWidgetItem | None) -> None:
        self.flush()
        p = cur.data(PRESET_ROLE) if cur is not None else None
        if p is not None:
            self._show(p)

    def _show(self, p: Preset | None) -> None:
        self.current = p
        for _it, card in self._items():
            card.set_selected(card.preset is p)
        self.detail.setVisible(p is not None)
        self.empty_detail.setVisible(p is None)
        if p is None:
            return
        editable = not p.builtin
        self._loading = True
        self.name_edit.setText(display_name(p))
        self.name_edit.setReadOnly(not editable)
        self.name_edit.setCursorPosition(0)
        set_badge(
            self.kind_badge, tr('Custom') if editable else tr('Template'), 'accent' if editable else 'cyan'
        )
        self.template_note.setVisible(not editable)
        self.delete_btn.setVisible(editable)
        self.dup_btn.setText(tr('Duplicate') if editable else tr('Duplicate to Edit'))
        self.panel.set_settings(p.settings)
        self.panel.widget().setEnabled(editable)
        self.panel.reset_btn.setVisible(editable)
        self.preview.set_settings(p.settings)
        self._loading = False
        self._dirty = False
        self._set_status(tr('✓ Saved') if editable else '')
        self._update_summary()
        self.apply_btn.setEnabled(not self._locked)

    def _set_status(self, text: str, tooltip: str = '', warning: bool = False) -> None:
        name = 'Warning' if warning else 'Success'
        if self.save_status.objectName() != name:
            self.save_status.setObjectName(name)
            self.save_status.style().unpolish(self.save_status)
            self.save_status.style().polish(self.save_status)
        self.save_status.setText(text)
        self.save_status.setToolTip(tooltip)

    def _update_summary(self) -> None:
        if self.current is None:
            return
        parts = describe_settings(self.panel.settings())
        self.summary.setText(
            tr('Changes: ') + ' · '.join(parts)
            if parts
            else tr('No adjustments — photos are exported unchanged.')
        )

    # Editing / auto-save ------------------------------------------------------------------
    def _panel_changed(self) -> None:
        s = self.panel.settings()
        self.preview.set_settings(s)
        self._update_summary()
        if self._loading or self.current is None or self.current.builtin:
            return
        self._dirty = True
        self._set_status(tr('Saving…'), warning=True)
        self._save_timer.start()

    def flush(self) -> None:
        """Write pending edits of the current setting to disk now."""
        self._save_timer.stop()
        p = self.current
        if not self._dirty or p is None or p.builtin or p.path is None:
            return
        self._dirty = False
        s = self.panel.settings()
        try:
            save_user_preset(self.user_preset_dir, p.name, s, path=p.path)
        except OSError as exc:
            self._dirty = True
            self._set_status(tr('⚠ Not saved'), str(exc), warning=True)
            log.warning('Could not save setting %s: %s', p.path, exc)
            return
        p.settings = s
        for _it, card in self._items():
            if card.preset is p:
                card.set_preset(p)
        self._set_status(
            tr('✓ Saved {time}', time=time.strftime('%H:%M:%S')), tr('Saved to {path}', path=p.path)
        )
        self.presets_changed.emit()

    def _rename(self) -> None:
        p = self.current
        if p is None or p.builtin:
            return
        new = self.name_edit.text().strip()
        if new == p.name:
            return
        if not new:
            self.name_edit.setText(p.name)
            return
        others = [q.name for q in self.presets if q is not p]
        if new.casefold() in {n.casefold() for n in others}:
            QMessageBox.warning(self, tr('Rename'), tr('A setting named “{name}” already exists.', name=new))
            self.name_edit.setText(p.name)
            return
        self.flush()
        try:
            q = rename_user_preset(p, new)
        except (OSError, ValueError) as exc:
            QMessageBox.warning(self, tr('Rename'), tr('Could not rename the setting:\n{error}', error=exc))
            self.name_edit.setText(p.name)
            return
        old = p.name
        self.reload(select=q.path)
        self.preset_renamed.emit(old, q.name)
        self.presets_changed.emit()

    # Actions ----------------------------------------------------------------------------------
    def create(self, settings: AdjustmentSettings, name: str) -> Preset | None:
        self.flush()
        name = unique_name(name, [p.name for p in self.presets])
        try:
            path = save_user_preset(self.user_preset_dir, name, settings.copy())
        except OSError as exc:
            QMessageBox.warning(
                self, tr('New Setting'), tr('Could not save the setting:\n{error}', error=exc)
            )
            return None
        self.search.clear()
        self.reload(select=path)
        self.presets_changed.emit()
        self.name_edit.setFocus()
        self.name_edit.selectAll()
        return self.current

    def duplicate_current(self) -> None:
        p = self.current
        if p is not None:
            self.create(
                self.panel.settings(), tr('{name} copy', name=p.name) if not p.builtin else display_name(p)
            )

    def delete_current(self) -> None:
        p = self.current
        if p is None or p.builtin:
            return
        question = tr('Delete the setting “{name}”?\nThis cannot be undone.', name=p.name)
        if QMessageBox.question(self, tr('Delete Setting'), question) != QMessageBox.StandardButton.Yes:
            return
        self._dirty = False
        self._save_timer.stop()
        user = [q for q in self.presets if not q.builtin]
        idx = user.index(p) if p in user else 0
        try:
            delete_user_preset(p)
        except OSError as exc:
            QMessageBox.warning(
                self, tr('Delete Setting'), tr('Could not delete the setting:\n{error}', error=exc)
            )
            return
        rest = [q for q in user if q is not p]
        nxt = rest[min(idx, len(rest) - 1)] if rest else None
        self.current = None
        self.reload(select=nxt.path if nxt else None)
        self.presets_changed.emit()

    def _apply(self) -> None:
        self.flush()
        if self.current is not None and not self._locked:
            self.apply_requested.emit(self.current)

    def _export(self) -> None:
        p = self.current
        if p is None:
            return
        self.flush()
        safe = ''.join(c if c.isalnum() or c in ' -_()' else '_' for c in p.name).strip() or 'setting'
        path, _ = QFileDialog.getSaveFileName(
            self, tr('Export Setting'), str(Path.home() / f'{safe}.json'), tr('Setting files') + ' (*.json)'
        )
        if not path:
            return
        try:
            save_settings_json(Path(path), self.panel.settings(), p.name)
        except OSError as exc:
            QMessageBox.warning(self, tr('Export Setting'), tr('Could not export:\n{error}', error=exc))

    def _import(self) -> None:
        path, _ = QFileDialog.getOpenFileName(
            self, tr('Import Setting'), str(Path.home()), tr('Setting files') + ' (*.json)'
        )
        if not path:
            return
        try:
            name, settings = load_settings_json(Path(path))
        except (OSError, ValueError) as exc:
            QMessageBox.warning(
                self, tr('Import Setting'), tr('This file is not a valid setting:\n{error}', error=exc)
            )
            return
        self.create(settings, name or Path(path).stem)

    def _open_folder(self) -> None:
        self.user_preset_dir.mkdir(parents=True, exist_ok=True)
        open_in_file_manager(self.user_preset_dir)

    # Preview sample ---------------------------------------------------------------------------
    def set_sample(self, path: Path | None) -> None:
        self.sample_path = path if path and Path(path).is_file() else None
        self.refresh_sample()

    def refresh_sample(self) -> None:
        path = self.sample_path or self._editor_photo()
        self.sample_reset_btn.setVisible(self.sample_path is not None)
        if path is None:
            self.sample_label.setText(tr('No sample photo — choose one, or open a folder in the Editor'))
        else:
            src = tr('sample') if self.sample_path else tr('from the Editor')
            self.sample_label.setText(f'{Path(path).name}  ·  {src}')
            self.sample_label.setToolTip(str(path))
        if path != getattr(self.preview, '_path', None):
            self.preview.set_source(path)
        if path is None:
            self.preview.view.set_message(tr('Choose a sample photo to preview this setting'))

    def _choose_sample(self) -> None:
        start = str(self.sample_path.parent) if self.sample_path else ''
        path, _ = QFileDialog.getOpenFileName(
            self, tr('Choose a sample photo'), start, tr('Images') + ' ' + IMAGE_FILTER
        )
        if path:
            self.set_sample(Path(path))

    def set_device(self, device: str, sr_cuda: bool) -> None:
        self.preview.set_device(device, sr_cuda)

    def set_locked(self, locked: bool) -> None:
        self._locked = locked
        self.apply_btn.setEnabled(not locked and self.current is not None)
        self.apply_btn.setToolTip(
            tr('A batch is running') if locked else tr('Apply this setting in the Editor and switch to it')
        )


class AccountPage(QWidget):
    """User name / password, "keep me signed in" and sign out."""

    sign_out_requested = Signal()
    account_changed = Signal()

    def __init__(self, auth: AuthStore, data_dir: Path, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.auth = auth
        self.data_dir = data_dir

        # Profile card
        profile = QFrame()
        profile.setObjectName('Card')
        self.avatar = QLabel()
        self.avatar.setObjectName('Avatar')
        self.avatar.setFixedSize(46, 46)
        self.avatar.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.avatar.setStyleSheet('border-radius: 23px; font-size: 15pt;')
        self.user_label = QLabel()
        self.user_label.setObjectName('Heading')
        self.default_badge = badge(tr('Default password — please change it'), 'warning')
        self.remember = QCheckBox(tr('Keep me signed in on this computer'))
        self.remember.setToolTip(tr('When off, the sign-in page appears every time the app starts'))
        self.sign_out_btn = button(tr('Sign out'), 'logout', 'Danger')
        pl = QVBoxLayout(profile)
        pl.setContentsMargins(20, 18, 20, 18)
        pl.setSpacing(12)
        r = QHBoxLayout()
        r.setSpacing(14)
        r.addWidget(self.avatar)
        col = QVBoxLayout()
        col.setSpacing(4)
        col.addWidget(self.user_label)
        brow = QHBoxLayout()
        brow.addWidget(self.default_badge)
        brow.addStretch(1)
        col.addLayout(brow)
        r.addLayout(col, 1)
        r.addWidget(self.sign_out_btn, 0, Qt.AlignmentFlag.AlignTop)
        pl.addLayout(r)
        pl.addWidget(self.remember)

        # Credentials card
        cred = QFrame()
        cred.setObjectName('Card')
        self.current_pw = QLineEdit()
        self.new_user = QLineEdit()
        self.new_pw = QLineEdit()
        self.confirm_pw = QLineEdit()
        for w in (self.current_pw, self.new_pw, self.confirm_pw):
            w.setEchoMode(QLineEdit.EchoMode.Password)
        self.new_pw.setPlaceholderText(tr('Leave empty to keep the current password'))
        self.confirm_pw.setPlaceholderText(tr('Type the new password again'))
        self.current_pw.setPlaceholderText(tr('Required to save changes'))
        self.result = QLabel()
        self.result.setWordWrap(True)
        self.result.hide()
        self.save_btn = button(tr('Save account'), 'save', 'Accent')
        cl = QVBoxLayout(cred)
        cl.setContentsMargins(20, 18, 20, 18)
        cl.setSpacing(10)
        t = QLabel(tr('SIGN-IN DETAILS'))
        t.setObjectName('SectionTitle')
        cl.addWidget(t)
        form = QFormLayout()
        form.setHorizontalSpacing(16)
        form.setVerticalSpacing(10)
        form.setLabelAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
        form.addRow(tr('User name'), self.new_user)
        form.addRow(tr('New password'), self.new_pw)
        form.addRow(tr('Confirm password'), self.confirm_pw)
        form.addRow(tr('Current password'), self.current_pw)
        cl.addLayout(form)
        cl.addWidget(self.result)
        srow = QHBoxLayout()
        srow.addStretch(1)
        srow.addWidget(self.save_btn)
        cl.addLayout(srow)

        # Storage card
        store = QFrame()
        store.setObjectName('Card')
        sl = QVBoxLayout(store)
        sl.setContentsMargins(20, 16, 20, 16)
        sl.setSpacing(8)
        t2 = QLabel(tr('LOCAL DATA'))
        t2.setObjectName('SectionTitle')
        info = QLabel(
            tr(
                'Your settings, the last used folder and the sign-in are saved on this computer '
                'and restored automatically when the app opens.'
            )
        )
        info.setObjectName('PageSubtitle')
        info.setWordWrap(True)
        self.path_label = QLabel(str(data_dir))
        self.path_label.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        open_btn = button(tr('Open data folder'), 'folder')
        prow = QHBoxLayout()
        prow.addWidget(self.path_label, 1)
        prow.addWidget(open_btn)
        sl.addWidget(t2)
        sl.addWidget(info)
        sl.addLayout(prow)

        inner = QWidget()
        inner.setMaximumWidth(640)
        il = QVBoxLayout(inner)
        il.setContentsMargins(0, 0, 0, 0)
        il.setSpacing(12)
        il.addWidget(profile)
        il.addWidget(cred)
        il.addWidget(store)
        il.addStretch(1)
        wrap = QWidget()
        wl = QHBoxLayout(wrap)
        wl.setContentsMargins(0, 0, 0, 0)
        wl.addWidget(inner, 1)
        wl.addStretch(0)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setWidget(wrap)
        scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.addWidget(scroll)

        self.remember.toggled.connect(self.auth.set_remembered)
        self.sign_out_btn.clicked.connect(self.sign_out_requested.emit)
        self.save_btn.clicked.connect(self._save)
        self.current_pw.returnPressed.connect(self._save)
        open_btn.clicked.connect(lambda: open_in_file_manager(self.data_dir))
        self.refresh()

    def refresh(self) -> None:
        name = self.auth.username
        self.avatar.setText(name[:1].upper())
        self.user_label.setText(name)
        self.default_badge.setVisible(self.auth.uses_default_credentials)
        self.remember.blockSignals(True)
        self.remember.setChecked(self.auth.is_remembered())
        self.remember.blockSignals(False)
        self.new_user.setText(name)
        for w in (self.current_pw, self.new_pw, self.confirm_pw):
            w.clear()

    def _show_result(self, ok: bool, text: str) -> None:
        self.result.setObjectName('Success' if ok else 'Error')
        self.result.setText(('✓ ' if ok else '✗ ') + text)
        self.result.style().unpolish(self.result)
        self.result.style().polish(self.result)
        self.result.show()

    def _save(self) -> None:
        if self.new_pw.text() != self.confirm_pw.text():
            self._show_result(False, tr('The new passwords do not match.'))
            return
        try:
            self.auth.change_credentials(self.current_pw.text(), self.new_user.text(), self.new_pw.text())
        except AuthError as exc:
            self._show_result(False, tr_msg(str(exc)))
            return
        self.refresh()
        self._show_result(True, tr('Account saved.'))
        self.account_changed.emit()


class GeneralPage(QWidget):
    """App-wide preferences: the interface language."""

    language_selected = Signal(str)  # "en" / "vi"

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        card = QFrame()
        card.setObjectName('Card')
        t = QLabel(tr('LANGUAGE'))
        t.setObjectName('SectionTitle')
        info = QLabel(
            tr(
                'Language of the menus, buttons and messages. The window is rebuilt right away; '
                'your folder, adjustments and settings are kept.'
            )
        )
        info.setObjectName('PageSubtitle')
        info.setWordWrap(True)
        self.combo = QComboBox()
        for code, name in LANGUAGES.items():
            self.combo.addItem(icon('globe'), name, code)
        self.combo.setCurrentIndex(max(0, self.combo.findData(language())))
        self.combo.setMinimumWidth(220)
        cl = QVBoxLayout(card)
        cl.setContentsMargins(20, 18, 20, 18)
        cl.setSpacing(10)
        cl.addWidget(t)
        cl.addWidget(info)
        form = QFormLayout()
        form.setHorizontalSpacing(16)
        form.addRow(tr('Language'), self.combo)
        cl.addLayout(form)

        inner = QWidget()
        inner.setMaximumWidth(640)
        il = QVBoxLayout(inner)
        il.setContentsMargins(0, 0, 0, 0)
        il.addWidget(card)
        il.addStretch(1)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.addWidget(inner, 1)
        lay.addStretch(0)
        self.combo.activated.connect(self._chosen)

    def _chosen(self, _index: int) -> None:
        code = self.combo.currentData()
        if code != language():
            self.language_selected.emit(code)

    def reset(self) -> None:
        """Show the active language again (after a refused change)."""
        self.combo.setCurrentIndex(max(0, self.combo.findData(language())))


class SettingsView(QWidget):
    """Settings page with three tabs: Filter Settings, Account and General."""

    TABS = 3

    def __init__(
        self,
        user_preset_dir: Path,
        auth: AuthStore,
        data_dir: Path,
        editor_settings: Callable[[], AdjustmentSettings],
        editor_photo: Callable[[], Path | None],
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self.setObjectName('Page')
        self.header = PageHeader(
            tr('Settings'),
            tr(
                'Saved filter settings are stored on this computer and '
                'ready in the Editor every time you open the app.'
            ),
        )
        self.filters = FilterSettingsPage(user_preset_dir, editor_settings, editor_photo)
        self.account = AccountPage(auth, data_dir)
        self.general = GeneralPage()
        self.tabs = QStackedWidget()
        self.tabs.addWidget(self.filters)
        self.tabs.addWidget(self.account)
        self.tabs.addWidget(self.general)
        self.tab_filters = button(tr('Filter Settings'), 'sliders', 'Tab')
        self.tab_account = button(tr('Account'), 'shield', 'Tab')
        self.tab_general = button(tr('General'), 'globe', 'Tab')
        self.tab_buttons = (self.tab_filters, self.tab_account, self.tab_general)
        group = QButtonGroup(self)
        for i, b in enumerate(self.tab_buttons):
            b.setCheckable(True)
            group.addButton(b)
            b.clicked.connect(lambda _=False, i=i: self.show_tab(i))
        self.tab_filters.setChecked(True)
        tabs = QHBoxLayout()
        tabs.setSpacing(0)
        for b in self.tab_buttons:
            tabs.addWidget(b)
        tabs.addStretch(1)
        line = QFrame()
        line.setFixedHeight(1)
        line.setStyleSheet('background-color: #1f2b45;')

        lay = QVBoxLayout(self)
        lay.setContentsMargins(22, 18, 22, 18)
        lay.setSpacing(10)
        lay.addWidget(self.header)
        lay.addLayout(tabs)
        lay.addWidget(line)
        lay.addSpacing(4)
        lay.addWidget(self.tabs, 1)

    def show_tab(self, index: int) -> None:
        self.filters.flush()
        index = max(0, min(self.TABS - 1, index))
        self.tabs.setCurrentIndex(index)
        self.tab_buttons[index].setChecked(True)
        if index == 1:
            self.account.refresh()
