"""Local sign-in (core.auth)."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from core.auth import AuthError, AuthStore


def test_default_account(tmp_path: Path):
    a = AuthStore(tmp_path / 'auth.json')
    assert a.username == 'admin' and a.uses_default_credentials and not a.is_remembered()
    assert a.verify('admin', 'admin') and a.verify(' Admin ', 'admin')
    assert not a.verify('admin', 'wrong') and not a.verify('root', 'admin')
    assert not (tmp_path / 'auth.json').exists()  # nothing written until a login


def test_login_remembered_across_restarts(tmp_path: Path):
    path = tmp_path / 'auth.json'
    assert not AuthStore(path).login('admin', 'nope', remember=True)
    assert AuthStore(path).login('admin', 'admin', remember=True)
    data = json.loads(path.read_text(encoding='utf-8'))
    assert 'admin' not in json.dumps(data['hash']) and data['remember'] is True
    b = AuthStore(path)
    assert b.is_remembered()
    b.logout()
    assert not AuthStore(path).is_remembered()
    assert AuthStore(path).login('admin', 'admin', remember=False)
    assert not AuthStore(path).is_remembered()


def test_change_credentials(tmp_path: Path):
    path = tmp_path / 'auth.json'
    a = AuthStore(path)
    with pytest.raises(AuthError):
        a.change_credentials('bad', 'admin', 'secret')
    with pytest.raises(AuthError):
        a.change_credentials('admin', '  ', 'secret')
    with pytest.raises(AuthError):
        a.change_credentials('admin', 'boss', 'abc')
    a.change_credentials('admin', ' boss ', 's3cret')
    b = AuthStore(path)
    assert b.username == 'boss' and not b.uses_default_credentials
    assert b.verify('boss', 's3cret') and not b.verify('admin', 'admin')
    b.change_credentials('s3cret', 'chief', '')  # empty new password keeps the current one
    assert AuthStore(path).verify('chief', 's3cret')


@pytest.mark.parametrize(
    'content',
    ['{broken', '[]', '{"username": "x"}', '{"username": "x", "salt": "zz", "hash": "a", "iterations": 5}'],
)
def test_broken_file_falls_back_to_default(tmp_path: Path, content: str):
    path = tmp_path / 'auth.json'
    path.write_text(content, encoding='utf-8')
    assert AuthStore(path).verify('admin', 'admin')
