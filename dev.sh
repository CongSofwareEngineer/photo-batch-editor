#!/usr/bin/env bash
# DEV (macOS / Linux): runs the app from source with live reload (dev.py) - no build.
# Save a .py file -> the app restarts on the same page; save ui/theme.qss -> applied instantly.
set -euo pipefail
cd "$(dirname "$0")"
if [ ! -x venv/bin/python ]; then
    echo "venv/ not found. Create it once with:"
    echo "  python3 -m venv venv && venv/bin/pip install -r requirements-dev.txt"
    exit 1
fi
exec venv/bin/python dev.py "$@"
