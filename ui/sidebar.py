"""Left navigation bar: brand, pages, signed-in user."""

from __future__ import annotations

from PySide6.QtCore import QSize, Qt, Signal
from PySide6.QtWidgets import (
    QButtonGroup,
    QFrame,
    QHBoxLayout,
    QLabel,
    QSizePolicy,
    QToolButton,
    QVBoxLayout,
    QWidget,
)

from core.i18n import tr
from ui.icons import icon
from ui.widgets import LogoMark, tool_button

PAGES = (
    ('editor', 'Batch edit', 'wand', 'Edit a folder of photos (Ctrl+1)'),
    ('photo', 'Photo editor', 'image', 'Edit one photo: layers, text, crop, blur, collage (Ctrl+2)'),
    ('video', 'Video editor', 'film', 'Cut a video, mute it, add text and music (Ctrl+3)'),
    ('settings', 'Settings', 'layers', 'Saved filter settings, account and language (Ctrl+4)'),
)


class Sidebar(QFrame):
    page_requested = Signal(str)
    sign_out_requested = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setObjectName('Sidebar')
        self.setFixedWidth(220)

        brand = QHBoxLayout()
        brand.setSpacing(10)
        brand.addWidget(LogoMark(38))
        col = QVBoxLayout()
        col.setSpacing(0)
        name = QLabel('Photo Batch')
        name.setObjectName('Brand')
        sub = QLabel('EDITOR  ·  GPU')
        sub.setObjectName('BrandSub')
        col.addWidget(name)
        col.addWidget(sub)
        brand.addLayout(col, 1)

        caption = QLabel(tr('WORKSPACE'))
        caption.setObjectName('NavCaption')
        self.group = QButtonGroup(self)
        self.group.setExclusive(True)
        self.buttons: dict[str, QToolButton] = {}
        nav = QVBoxLayout()
        nav.setSpacing(4)
        for key, text, icon_name, tip in PAGES:
            b = QToolButton()
            b.setObjectName('NavButton')
            b.setText(tr(text))
            b.setIcon(icon(icon_name))
            b.setIconSize(QSize(18, 18))
            b.setToolTip(tr(tip))
            b.setCheckable(True)
            b.setToolButtonStyle(Qt.ToolButtonStyle.ToolButtonTextBesideIcon)
            b.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
            b.setCursor(Qt.CursorShape.PointingHandCursor)
            b.clicked.connect(lambda _=False, k=key: self.page_requested.emit(k))
            self.group.addButton(b)
            self.buttons[key] = b
            nav.addWidget(b)

        self.status = QLabel()
        self.status.setObjectName('PageSubtitle')
        self.status.setWordWrap(True)
        self.status.hide()

        user = QFrame()
        user.setObjectName('UserCard')
        self.avatar = QLabel()
        self.avatar.setObjectName('Avatar')
        self.avatar.setFixedSize(34, 34)
        self.avatar.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.user_name = QLabel()
        self.user_name.setObjectName('UserName')
        role = QLabel(tr('Signed in'))
        role.setObjectName('PageSubtitle')
        self.sign_out_btn = tool_button('logout', tr('Sign out'))
        self.sign_out_btn.setFixedSize(32, 32)  # leaves room for longer translations under the name
        self.sign_out_btn.setStyleSheet('QToolButton { padding: 6px; }')
        ul = QHBoxLayout(user)
        ul.setContentsMargins(10, 8, 6, 8)
        ul.setSpacing(10)
        ul.addWidget(self.avatar)
        ucol = QVBoxLayout()
        ucol.setSpacing(0)
        ucol.addWidget(self.user_name)
        ucol.addWidget(role)
        ul.addLayout(ucol, 1)
        ul.addWidget(self.sign_out_btn)

        lay = QVBoxLayout(self)
        lay.setContentsMargins(14, 18, 14, 14)
        lay.setSpacing(8)
        lay.addLayout(brand)
        lay.addSpacing(22)
        lay.addWidget(caption)
        lay.addLayout(nav)
        lay.addStretch(1)
        lay.addWidget(self.status)
        lay.addWidget(user)
        self.sign_out_btn.clicked.connect(self.sign_out_requested.emit)

    def set_page(self, key: str) -> None:
        self.buttons[key].setChecked(True)

    def set_user(self, name: str) -> None:
        self.avatar.setText(name[:1].upper() or '?')
        self.user_name.setText(name)
        self.user_name.setToolTip(name)

    def set_status(self, text: str) -> None:
        self.status.setText(text)
        self.status.setVisible(bool(text))
