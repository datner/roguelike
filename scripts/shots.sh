#!/usr/bin/env bash
# Renders each gui screen (menu, play, panels, cursors, effects, sprites) to
# PNGs, so drawing can be looked at without opening the window. Needs a GPU;
# not part of check.sh. Prints the directory.
# Usage: scripts/shots.sh [dir]   (default: $TMPDIR/roguelike-shots)
set -euo pipefail
dir="${1:-${TMPDIR:-/tmp}/roguelike-shots}"
mkdir -p "$dir"
GUI_SCREENSHOTS="$dir" cargo test -q -p gui screenshots -- --ignored >/dev/null
echo "$dir"
