#!/usr/bin/env bash
# Reads a long markdown document a part at a time (docs/vision.md, a plan), so a session learns
# what it holds from its outline and reads only the parts it needs. Assumes no format beyond
# headings: a part is a heading and everything under it.
# Usage:
#   doc.sh FILE            outline: each heading with its size and its opening words
#   doc.sh FILE PART...    those parts, each from its heading to the next at its level or above.
#                          PART is a numbered heading's number ("3", "1b") or else any words of
#                          a heading ("fights", "judgement"); case does not matter.
set -euo pipefail

# The heading's level (count of leading #), 0 for a line that is no heading.
level() {
  local hashes=${1%%[!#]*}
  [[ $1 == "$hashes "* ]] && echo ${#hashes} || echo 0
}

outline() {
  local file=$1 line n=0 head="" start=0 lead="" l
  while IFS= read -r line; do
    n=$((n + 1))
    l=$(level "$line")
    if ((l)); then
      [[ -z $head ]] || echo "$head ($((n - start)) lines)${lead:+ — $lead}"
      head="$(printf '%*s' $(((l - 1) * 2)) '')$line" start=$n lead=""
    elif [[ -n $head && -z $lead && -n $line ]]; then
      # A section's opening words say what it is about; markup and list marks only cost width.
      lead=${line#- }
      lead=${lead//\*\*/}
      ((${#lead} <= 90)) || lead="${lead:0:89}…"
    fi
  done <"$file"
  [[ -z $head ]] || echo "$head ($((n + 1 - start)) lines)${lead:+ — $lead}"
}

part() {
  local file=$1 want=${2,,} line l printing=0 found=0 title
  local numbered=0
  [[ $want =~ ^[0-9][0-9a-z+]*$ ]] && numbered=1
  while IFS= read -r line; do
    l=$(level "$line")
    if ((printing)); then
      if ((l && l <= printing)); then
        printing=0
      else
        echo "$line"
        continue
      fi
    fi
    ((l)) || continue
    title=${line#"${line%%[!#]*}" }
    title=${title,,}
    if { ((numbered)) && [[ $title == "$want. "* || $title == "$want "* ]]; } ||
      { ((!numbered)) && [[ $title == *"$want"* ]]; }; then
      printing=$l found=1
      echo "$line"
    fi
  done <"$1"
  ((found)) || echo "$file: no part '$2' (doc.sh $file lists them)"
}

file=${1:?usage: doc.sh FILE [PART...]}
[[ -f $file ]] || { echo "no file $file"; exit 1; }
shift
if (($# == 0)); then
  outline "$file"
else
  for want in "$@"; do part "$file" "$want"; done
fi
