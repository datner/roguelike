#!/usr/bin/env bash
# The single "is it done?" gate for humans and agents.
# Quiet on success (one line per step); on failure prints only the failing
# step's output, capped, so agent context isn't flooded.
set -euo pipefail
cd "$(dirname "$0")/.."

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
    failed=1
  fi
}
# Every step runs, so one run shows every problem.
failed=0

# ADR 0001: game (and front, ADR 0020) must not depend on terminal/graphics crates.
boundary() {
  local forbidden='^(crossterm|ratatui|termion|termwiz|console|bracket-lib|bracket-terminal|macroquad|bevy|wgpu|winit|sdl2|minifb|pixels|egui|eframe|epaint|egui-wgpu|egui_glow|glow) '
  local bad crate
  # front is shared by every frontend (ADR 0020), so it draws nothing either.
  for crate in game front; do
    bad=$(cargo tree -p "$crate" -e no-dev --prefix none --format '{p}' | grep -E "$forbidden" | sort -u || true)
    if [[ -n "$bad" ]]; then
      echo "$crate depends on terminal/graphics crates:"
      echo "$bad"
      return 1
    fi
  done
  local names
  for crate in front gui; do
    # The ECS is game's internals (ADR 0008); frontends go through game functions.
    if cargo tree -p "$crate" -e normal --depth 1 --prefix none --format '{p}' | grep -qE '^hecs ' ||
      grep -rqw hecs "$crate/src"; then
      echo "$crate refers to hecs; expose what it needs through game instead"
      return 1
    fi
    # Frontends send intent (`PlayerAction`) only; game resolves actors and commands (ADR 0001).
    names=$(grep -rnwE 'Command|Entity' "$crate/src" || true)
    if [[ -n "$names" ]]; then
      echo "$crate names game's Command or Entity; send a PlayerAction instead:"
      echo "$names"
      return 1
    fi
  done
}

# docs/MAP.md is what sessions orient from, so a stale one misleads. It is
# generated, so a stale one is rewritten (commit it with the change), not failed.
map() {
  local tmp
  tmp=$(mktemp)
  trap 'rm -f "$tmp"' RETURN
  scripts/map.sh "$tmp"
  if ! cmp -s "$tmp" docs/MAP.md; then
    cp "$tmp" docs/MAP.md
    echo "note docs/MAP.md was stale: rewritten, commit it with the change"
  fi
}

# File names only; `cargo fmt` fixes them, so the diff is noise.
run fmt      cargo fmt --all --check -- -l
map # Prints only when it rewrote the map.
run boundary boundary
# docs/plans/README.md: every game-system plan says what it reads and what reads it.
run connections bash -c 'scripts/connections.sh --test && scripts/connections.sh'
run clippy   cargo clippy --workspace --all-targets --quiet -- -D warnings
run test     cargo test --workspace --quiet
exit "$failed"
