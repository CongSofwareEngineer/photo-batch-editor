"""Sign-in screen shown until the user logs in with "Keep me signed in"."""

from __future__ import annotations

import math

from PySide6.QtCore import QPointF, QRectF, QSize, Qt, QTimer, Signal
from PySide6.QtGui import QAction, QColor, QPainter, QPaintEvent, QPen, QRadialGradient
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QFrame,
    QGraphicsDropShadowEffect,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QVBoxLayout,
    QWidget,
)

from core.auth import DEFAULT_PASSWORD, DEFAULT_USERNAME, AuthStore
from core.i18n import LANGUAGES, language, tr
from ui.icons import icon
from ui.widgets import LogoMark, button


class LoginView(QWidget):
    """Full-window sign-in page. ``logged_in`` fires after a successful sign-in."""

    logged_in = Signal()
    language_selected = Signal(str)  # "en" / "vi"

    def __init__(self, auth: AuthStore, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.auth = auth
        self.setObjectName('Page')
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, False)

        card = QFrame()
        card.setObjectName('LoginCard')
        card.setFixedWidth(420)
        card.setStyleSheet(
            'QFrame#LoginCard { background-color: rgba(16, 23, 42, 235); border: 1px solid #24314f;'
            ' border-radius: 18px; } QFrame#LoginCard QLabel, QFrame#LoginCard QCheckBox'
            ' { background: transparent; }'
        )
        shadow = QGraphicsDropShadowEffect(card)
        shadow.setBlurRadius(60)
        shadow.setOffset(0, 18)
        shadow.setColor(QColor(0, 0, 0, 170))
        card.setGraphicsEffect(shadow)

        logo = LogoMark(56)
        title = QLabel('Photo Batch Editor')
        title.setObjectName('PageTitle')
        title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        sub = QLabel(tr('Sign in to continue'))
        sub.setObjectName('PageSubtitle')
        sub.setAlignment(Qt.AlignmentFlag.AlignCenter)

        self.username = QLineEdit()
        self.username.setObjectName('Large')
        self.username.setPlaceholderText(tr('User name'))
        self.username.addAction(icon('user', '#6f80a3'), QLineEdit.ActionPosition.LeadingPosition)
        self.password = QLineEdit()
        self.password.setObjectName('Large')
        self.password.setPlaceholderText(tr('Password'))
        self.password.setEchoMode(QLineEdit.EchoMode.Password)
        self.password.addAction(icon('lock', '#6f80a3'), QLineEdit.ActionPosition.LeadingPosition)
        self.reveal = QAction(icon('eye', '#6f80a3'), tr('Show password'), self.password)
        self.password.addAction(self.reveal, QLineEdit.ActionPosition.TrailingPosition)
        self.remember = QCheckBox(tr('Keep me signed in'))
        self.remember.setChecked(True)
        self.remember.setToolTip(tr('Next time the app opens directly, without asking for the password'))
        self.error = QLabel()
        self.error.setObjectName('Error')
        self.error.setWordWrap(True)
        self.error.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.error.hide()
        self.sign_in = button(tr('Sign in'), None, 'Primary')
        self.sign_in.setDefault(True)
        self.hint = QLabel(
            tr(
                'Default account: <b>{user}</b> / <b>{password}</b> — change it in Settings › Account',
                user=DEFAULT_USERNAME,
                password=DEFAULT_PASSWORD,
            )
        )
        self.hint.setObjectName('PageSubtitle')
        self.hint.setWordWrap(True)
        self.hint.setAlignment(Qt.AlignmentFlag.AlignCenter)

        lay = QVBoxLayout(card)
        lay.setContentsMargins(36, 34, 36, 30)
        lay.setSpacing(12)
        row = QHBoxLayout()
        row.addStretch(1)
        row.addWidget(logo)
        row.addStretch(1)
        lay.addLayout(row)
        lay.addSpacing(4)
        lay.addWidget(title)
        lay.addWidget(sub)
        lay.addSpacing(14)
        for text, w in (('USER NAME', self.username), ('PASSWORD', self.password)):
            cap = QLabel(tr(text))
            cap.setObjectName('SectionTitle')
            lay.addWidget(cap)
            lay.addWidget(w)
        lay.addSpacing(2)
        lay.addWidget(self.remember)
        lay.addWidget(self.error)
        lay.addSpacing(6)
        lay.addWidget(self.sign_in)
        lay.addSpacing(4)
        lay.addWidget(self.hint)

        outer = QVBoxLayout(self)
        outer.addStretch(1)
        mid = QHBoxLayout()
        mid.addStretch(1)
        mid.addWidget(card)
        mid.addStretch(1)
        outer.addLayout(mid)
        outer.addStretch(1)
        foot = QLabel(tr('GPU-accelerated batch photo editing'))
        foot.setObjectName('PageSubtitle')
        self.lang = QComboBox()
        for code, name in LANGUAGES.items():
            self.lang.addItem(icon('globe'), name, code)
        self.lang.setCurrentIndex(max(0, self.lang.findData(language())))
        self.lang.setToolTip('Language / Ngôn ngữ')
        frow = QHBoxLayout()
        frow.addStretch(1)
        frow.addWidget(foot)
        frow.addStretch(1)
        outer.addLayout(frow)
        corner = QHBoxLayout()
        corner.addStretch(1)
        corner.addWidget(self.lang)
        outer.insertLayout(0, corner)
        outer.setContentsMargins(24, 24, 24, 18)

        self.sign_in.clicked.connect(self.try_login)
        self.username.returnPressed.connect(self._username_enter)
        self.password.returnPressed.connect(self.try_login)
        self.reveal.triggered.connect(self._toggle_reveal)
        self.lang.activated.connect(lambda _i: self.language_selected.emit(self.lang.currentData()))
        self.username.textEdited.connect(lambda _t: self.error.hide())
        self.password.textEdited.connect(lambda _t: self.error.hide())
        self.reset()

    # API
    def reset(self) -> None:
        """Prepare for a (new) sign-in: user name pre-filled, password cleared."""
        self.username.setText(self.auth.username)
        self.password.clear()
        self.error.hide()
        self.hint.setVisible(self.auth.uses_default_credentials)
        self.sign_in.setEnabled(True)
        self.sign_in.setText(tr('Sign in'))
        QTimer.singleShot(0, self.focus_first)

    def focus_first(self) -> None:
        (self.password if self.username.text() else self.username).setFocus()

    def try_login(self) -> None:
        user, pw = self.username.text().strip(), self.password.text()
        if not user or not pw:
            self._fail(tr('Enter your user name and password.'))
            return
        self.sign_in.setEnabled(False)
        self.sign_in.setText(tr('Signing in…'))
        self.sign_in.repaint()
        ok = self.auth.login(user, pw, self.remember.isChecked())
        self.sign_in.setEnabled(True)
        self.sign_in.setText(tr('Sign in'))
        if not ok:
            self._fail(tr('Incorrect user name or password.'))
            self.password.selectAll()
            self.password.setFocus()
            return
        self.password.clear()
        self.logged_in.emit()

    # Internals
    def _fail(self, text: str) -> None:
        self.error.setText(text)
        self.error.show()

    def _username_enter(self) -> None:
        self.password.setFocus()

    def _toggle_reveal(self) -> None:
        hidden = self.password.echoMode() == QLineEdit.EchoMode.Password
        self.password.setEchoMode(QLineEdit.EchoMode.Normal if hidden else QLineEdit.EchoMode.Password)
        self.reveal.setIcon(icon('eye-off' if hidden else 'eye', '#6f80a3'))
        self.reveal.setText(tr('Hide password') if hidden else tr('Show password'))

    def paintEvent(self, _e: QPaintEvent) -> None:  # noqa: N802
        """Dark backdrop with a faint grid and two soft glows."""
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        r = QRectF(self.rect())
        p.fillRect(r, QColor('#080c19'))
        for center, radius, color in (
            (QPointF(r.width() * 0.18, r.height() * 0.2), 0.55, QColor(79, 140, 255, 70)),
            (QPointF(r.width() * 0.85, r.height() * 0.85), 0.6, QColor(139, 92, 246, 60)),
            (QPointF(r.width() * 0.6, r.height() * 0.05), 0.35, QColor(34, 211, 238, 35)),
        ):
            g = QRadialGradient(center, max(r.width(), r.height()) * radius)
            g.setColorAt(0, color)
            g.setColorAt(1, QColor(color.red(), color.green(), color.blue(), 0))
            p.fillRect(r, g)
        step = 36
        pen = QPen(QColor(120, 150, 220, 16))
        pen.setWidth(1)
        p.setPen(pen)
        for x in range(0, int(r.width()) + step, step):
            p.drawLine(x, 0, x, int(r.height()))
        for y in range(0, int(r.height()) + step, step):
            p.drawLine(0, y, int(r.width()), y)
        # a few brighter grid nodes
        p.setPen(Qt.PenStyle.NoPen)
        for i in range(24):
            x = (i * 7 % 23) / 23 * r.width()
            y = (i * 11 % 17) / 17 * r.height()
            x, y = round(x / step) * step, round(y / step) * step
            alpha = 40 + int(50 * abs(math.sin(i)))
            p.setBrush(QColor(34, 211, 238, alpha))
            p.drawEllipse(QPointF(x, y), 1.6, 1.6)

    def sizeHint(self) -> QSize:  # noqa: N802
        return QSize(900, 650)
