#!/usr/bin/env bash
# The single "is it done?" gate for humans and agents.
# Quiet on success (one line per step); on failure prints only the failing
# step's output, capped, so agent context isn't flooded.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v cargo >/dev/null 2>&1; then
  exec nix develop --command "$0" "$@"
fi

MAX_LINES=${CHECK_MAX_LINES:-120}

run() {
  local name=$1
  shift
  local out
  if out=$("$@" 2>&1); then
    echo "ok   $name"
  else
    echo "FAIL $name"
    echo "$out" | head -n "$MAX_LINES"
    exit 1
  fi
}

# ADR 0001: game must not depend on terminal/graphics crates, and term must
# reach crossterm only through ratatui::crossterm.
boundary() {
  local forbidden='^(crossterm|ratatui|termion|termwiz|console|bracket-lib|bracket-terminal|macroquad|bevy|wgpu|winit|sdl2|minifb|pixels) '
  local bad
  bad=$(cargo tree -p game -e no-dev --prefix none --format '{p}' | grep -E "$forbidden" | sort -u || true)
  if [[ -n "$bad" ]]; then
    echo "game depends on terminal/graphics crates:"
    echo "$bad"
    return 1
  fi
  if cargo tree -p term -e normal --depth 1 --prefix none --format '{p}' | grep -qE '^crossterm '; then
    echo "term depends on crossterm directly; use ratatui::crossterm instead"
    return 1
  fi
}

run fmt      cargo fmt --all --check
run boundary boundary
run clippy   cargo clippy --workspace --all-targets --quiet -- -D warnings
run test     cargo test --workspace --quiet
