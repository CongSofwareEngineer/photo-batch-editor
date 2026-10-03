"""Local sign-in: one account stored as a salted PBKDF2 hash in ``auth.json``.

This is a convenience lock for a desktop app, not a security boundary: the file lives in
the user's own profile. The default account is ``admin`` / ``admin``.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import logging
import os
from pathlib import Path
from typing import Any

log = logging.getLogger(__name__)

DEFAULT_USERNAME = 'admin'
DEFAULT_PASSWORD = 'admin'
ITERATIONS = 120_000
AUTH_FORMAT_VERSION = 1


def hash_password(password: str, salt: bytes, iterations: int = ITERATIONS) -> str:
    return hashlib.pbkdf2_hmac('sha256', password.encode('utf-8'), salt, iterations).hex()


class AuthError(ValueError):
    """A credentials change was rejected; the message is shown to the user."""


class AuthStore:
    """Reads/writes ``auth.json``. A missing or broken file means the default account."""

    def __init__(self, path: Path) -> None:
        self.path = Path(path)
        self._data = self._load()

    # Persistence ------------------------------------------------------------------------------
    @staticmethod
    def _default_data() -> dict[str, Any]:
        salt = os.urandom(16)
        return {
            'version': AUTH_FORMAT_VERSION,
            'username': DEFAULT_USERNAME,
            'salt': salt.hex(),
            'iterations': ITERATIONS,
            'hash': hash_password(DEFAULT_PASSWORD, salt),
            'remember': False,
            'is_default': True,
        }

    def _load(self) -> dict[str, Any]:
        try:
            data = json.loads(self.path.read_text(encoding='utf-8'))
        except (OSError, ValueError):
            return self._default_data()
        ok = (
            isinstance(data, dict)
            and isinstance(data.get('username'), str)
            and data['username']
            and isinstance(data.get('salt'), str)
            and isinstance(data.get('hash'), str)
            and isinstance(data.get('iterations'), int)
            and data['iterations'] > 0
        )
        if not ok:
            log.warning('Invalid %s, using the default account', self.path)
            return self._default_data()
        try:
            bytes.fromhex(data['salt'])
        except ValueError:
            return self._default_data()
        data['remember'] = data.get('remember') is True
        data['is_default'] = data.get('is_default') is True
        return data

    def _save(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_name(self.path.name + '.tmp')
        tmp.write_text(json.dumps(self._data, indent=2), encoding='utf-8')
        tmp.replace(self.path)

    # API ----------------------------------------------------------------------------------------
    @property
    def username(self) -> str:
        return self._data['username']

    @property
    def uses_default_credentials(self) -> bool:
        return bool(self._data.get('is_default'))

    def is_remembered(self) -> bool:
        return bool(self._data.get('remember'))

    def verify(self, username: str, password: str) -> bool:
        if username.strip().casefold() != self.username.casefold():
            # still hash, so a wrong user name takes as long as a wrong password
            hash_password(password, b'0' * 16, self._data['iterations'])
            return False
        digest = hash_password(password, bytes.fromhex(self._data['salt']), self._data['iterations'])
        return hmac.compare_digest(digest, self._data['hash'])

    def login(self, username: str, password: str, remember: bool) -> bool:
        if not self.verify(username, password):
            return False
        self.set_remembered(remember)
        return True

    def set_remembered(self, remember: bool) -> None:
        self._data['remember'] = bool(remember)
        try:
            self._save()
        except OSError:
            log.warning('Could not save %s', self.path, exc_info=True)

    def logout(self) -> None:
        self.set_remembered(False)

    def change_credentials(self, current_password: str, new_username: str, new_password: str) -> None:
        """Change the user name and/or password; raises ``AuthError`` with a user-facing message."""
        new_username = new_username.strip()
        if not self.verify(self.username, current_password):
            raise AuthError('The current password is incorrect.')
        if not new_username:
            raise AuthError('The user name cannot be empty.')
        if len(new_username) > 64:
            raise AuthError('The user name is too long (64 characters max).')
        if new_password and len(new_password) < 4:
            raise AuthError('The new password must have at least 4 characters.')
        salt = os.urandom(16)
        password = new_password or current_password
        data = dict(self._data)
        data.update(
            {
                'username': new_username,
                'salt': salt.hex(),
                'iterations': ITERATIONS,
                'hash': hash_password(password, salt),
                'is_default': new_username == DEFAULT_USERNAME and password == DEFAULT_PASSWORD,
            }
        )
        old, self._data = self._data, data
        try:
            self._save()
        except OSError as exc:
            self._data = old
            raise AuthError(f'Could not save the account: {exc}') from exc
