#!/usr/bin/env bash
# The balance bench (docs/plans/balance.md): every build in bench/builds
# against every scenario in bench/scenarios.ron, through the real game.
#   scripts/balance.sh            rewrites bench/report.md
#   scripts/balance.sh --check    regenerates to a scratch file and diffs it
#                                 against bench/report.md (exit 1 if moved)
#   scripts/balance.sh --print    prints the report instead
# Extra arguments go to the bench (e.g. --seeds 5 for a quick look).
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v cargo >/dev/null 2>&1; then
  exec nix develop --command "$0" "$@"
fi

mode=write
case ${1:-} in
--check | --print)
  mode=${1#--}
  shift
  ;;
esac

bench() { cargo run --quiet --release -p bench -- "$@"; }

case $mode in
write) bench --out bench/report.md "$@" ;;
print) bench "$@" ;;
check)
  tmp=$(mktemp)
  trap 'rm -f "$tmp"' EXIT
  bench --out "$tmp" "$@"
  if ! diff -u bench/report.md "$tmp"; then
    echo "balance moved: run scripts/balance.sh and commit bench/report.md with the change"
    exit 1
  fi
  ;;
esac
