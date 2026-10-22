#!/usr/bin/env bash
# Renders gui screens to PNGs, so drawing can be looked at without opening
# the window, or checks them against their references. Needs a GPU; not part
# of check.sh.
#
#   scripts/shots.sh <dir> [shot...] [--crop map|side|bottom]
#       Renders into <dir> and prints the PNGs written.
#   scripts/shots.sh --check [shot...]
#       Compares with gui/tests/snapshots/; on a mismatch prints the
#       <shot>.diff.png to look at, and fails.
#   scripts/shots.sh --update [shot...]
#       Rewrites the references that changed (commit them with the change).
#
# shots: menu play inventory look spells aim effects sprites fx (default all);
# fx is the effects animating, 100 ms a frame, left to right. Reading an image
# costs about width × height / 750 tokens: render only what a change touches,
# cropped to the region it touched.
set -euo pipefail
all=(menu play inventory look spells aim effects sprites fx)
usage="usage: scripts/shots.sh <dir>|--check|--update [shot...] [--crop map|side|bottom]"
case ${1:?$usage} in
--check | --update)
  mode=${1#--}
  dir=$(mktemp -d)
  ;;
*)
  mode=render
  dir=$1
  ;;
esac
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
((${#shots[@]})) || shots=("${all[@]}")
mkdir -p "$dir"
env=(GUI_SCREENSHOTS="$dir" GUI_CROP="$crop" GUI_SHOTS="$(
  IFS=,
  echo "${shots[*]}"
)")
case $mode in
check) env+=(GUI_CHECK=1 GUI_CROP=all) ;;
update) env+=(GUI_CHECK=1 GUI_CROP=all UPDATE_SNAPSHOTS=1) ;;
esac
if ! out=$(env "${env[@]}" cargo test -q -p gui screenshots -- --ignored 2>&1); then
  grep -E "^[a-z]+: '.*' Image did not match|panicked at|^no " <<<"$out" | grep -v "panicked at" >&2 ||
    echo "$out" | tail -20 >&2
  exit 1
fi
case $mode in
render) for shot in "${shots[@]}"; do echo "$dir/$shot.png"; done ;;
*) echo "ok: ${shots[*]}" ;;
esac
