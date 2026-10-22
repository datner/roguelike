#!/usr/bin/env bash
# Renders gui screens to PNGs, so drawing can be looked at without opening
# the window. Needs a GPU; not part of check.sh. Prints the PNGs written.
#
# Usage: scripts/shots.sh <dir> [shot...] [--crop map|side|bottom]
#   shots: menu play inventory look spells aim effects sprites fx (default: all)
#   fx: frames of the effects animating, 100 ms apart, left to right
# Reading an image costs about width × height / 750 tokens: render only the
# shots a change touches, cropped to the region it touched.
set -euo pipefail
dir=${1:?usage: scripts/shots.sh <dir> [shot...] [--crop map|side|bottom]}
shift
shots=()
crop=all
while (($#)); do
  case $1 in
  --crop)
    crop=${2:?--crop needs map, side or bottom}
    shift 2
    ;;
  *)
    shots+=("$1")
    shift
    ;;
  esac
done
mkdir -p "$dir"
names=$(
  IFS=,
  echo "${shots[*]:-}"
)
if ! out=$(GUI_SCREENSHOTS=$dir GUI_SHOTS=$names GUI_CROP=$crop \
  cargo test -q -p gui screenshots -- --ignored 2>&1); then
  grep -A2 panicked <<<"$out" >&2 || echo "$out" | tail -20 >&2
  exit 1
fi
for shot in "${shots[@]:-menu play inventory look spells aim effects sprites fx}"; do
  for name in $shot; do echo "$dir/$name.png"; done
done
