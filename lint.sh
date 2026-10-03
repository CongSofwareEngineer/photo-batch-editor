#!/usr/bin/env bash
# Like "eslint": format + lint + type check all Python code (rules: pyproject.toml).
# VS Code shows the same problems while typing and formats + fixes on save.
#   ./lint.sh          fix: format files, auto-fix lint problems, then type check
#   ./lint.sh --check  only check (no changes), exit code 1 if something is wrong
set -euo pipefail
cd "$(dirname "$0")"
PY=venv/bin/python
if [ "${1:-}" = "--check" ]; then
    "$PY" -m ruff format --check .
    "$PY" -m ruff check .
else
    "$PY" -m ruff format .
    "$PY" -m ruff check --fix .
fi
echo "=== Type check (Pyright) ==="
"$PY" -m pyright
