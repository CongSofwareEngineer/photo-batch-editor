"""Undo / redo by snapshots (photo and video editors).

The editors keep their whole document state in small objects; before a change the current
state is pushed here. Consecutive changes with the same ``key`` inside ``MERGE_SECONDS``
(one slider drag, typing in a box) become a single step.
"""

from __future__ import annotations

import time
from typing import Generic, TypeVar

T = TypeVar('T')

DEFAULT_LIMIT = 50
MERGE_SECONDS = 1.0


class History(Generic[T]):
    def __init__(self, limit: int = DEFAULT_LIMIT) -> None:
        self.limit = max(1, limit)
        self._undo: list[tuple[str, T]] = []
        self._redo: list[tuple[str, T]] = []
        self._last_key: str | None = None
        self._last_time = 0.0

    def push(self, label: str, state: T, key: str | None = None, now: float | None = None) -> bool:
        """Record ``state`` (the state *before* the change). Returns ``False`` when merged."""
        now = time.monotonic() if now is None else now
        merged = (
            key is not None
            and key == self._last_key
            and now - self._last_time < MERGE_SECONDS
            and bool(self._undo)
        )
        self._last_key, self._last_time = key, now
        self._redo.clear()
        if merged:
            return False
        self._undo.append((label, state))
        del self._undo[: -self.limit]
        return True

    def break_merge(self) -> None:
        self._last_key = None

    def undo(self, current: T) -> T | None:
        if not self._undo:
            return None
        label, state = self._undo.pop()
        self._redo.append((label, current))
        self._last_key = None
        return state

    def redo(self, current: T) -> T | None:
        if not self._redo:
            return None
        label, state = self._redo.pop()
        self._undo.append((label, current))
        self._last_key = None
        return state

    def can_undo(self) -> bool:
        return bool(self._undo)

    def can_redo(self) -> bool:
        return bool(self._redo)

    def undo_label(self) -> str:
        return self._undo[-1][0] if self._undo else ''

    def redo_label(self) -> str:
        return self._redo[-1][0] if self._redo else ''

    def clear(self) -> None:
        self._undo.clear()
        self._redo.clear()
        self._last_key = None

    def __len__(self) -> int:
        return len(self._undo)
