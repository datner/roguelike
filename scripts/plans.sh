#!/usr/bin/env bash
# Reads build plans (docs/plans/, format in its README) a piece at a time, so a session never reads
# a whole plan to learn what it is, where it stands, or what one slice asks.
# Usage:
#   plans.sh                      every plan: status and summary; open slices listed under it
#   plans.sh NAME                 one plan's header lines, then its outline (scripts/doc.sh)
#   plans.sh NAME PART...         just those parts: a slice by its number ("3", "1b"), a section by
#                                 words of its heading ("goal", "out of scope")
#   plans.sh --lint [--test]      check every plan follows the format (fixtures with --test)
# NAME is a file stem (`fights-2`).
set -euo pipefail
cd "$(dirname "$0")/.."

dir=docs/plans
done_re=' — done( \(.*\))?$'
slice_re='^### [0-9][0-9a-z +]*\. .* — (done|open)( \(.*\))?$'

plans() {
  local plan
  for plan in "$dir"/*.md; do
    [[ $plan == "$dir/README.md" ]] || echo "$plan"
  done
}

# Slice headings, without the `### `.
slices() {
  rg -N '^### ' -r '' "$1" || true
}

index() {
  local plan name summary total open status
  for plan in $(plans); do
    name=$(basename "$plan" .md)
    summary=$(rg -m1 -N '^Summary: (.*)$' -r '$1' "$plan" || echo '?')
    total=$(slices "$plan" | wc -l | tr -d ' ')
    open=$(slices "$plan" | rg -v -- "$done_re" || true)
    if [[ -z $open ]]; then
      status=done
    else
      status="$((total - $(echo "$open" | wc -l)))/$total"
    fi
    printf '%-16s %-6s %s\n' "$name" "$status" "$summary"
    [[ -z $open ]] || echo "$open" | sed 's/^/    /'
  done
}

outline() {
  # The header (Summary, Read first) whole, since the outline cuts a section's opening short.
  rg -N -m1 -B100 '^## ' "$1" | rg -v '^(## |$)' || true
  scripts/doc.sh "$1" | tail -n +2
}

# Prints what one plan breaks of the format, nothing if it follows it.
lacks() {
  local plan=$1 line in_slices=0 n=0
  [[ $(head -n 1 "$plan") == '# Plan: '* ]] || echo "$plan: first line is not '# Plan: <title>'"
  rg -q '^Summary: \S' "$plan" || echo "$plan: no 'Summary: <one sentence>' line"
  rg -q '^## Goal$' "$plan" || echo "$plan: no '## Goal' section"
  rg -q '^## Slices$' "$plan" || echo "$plan: no '## Slices' section"
  while IFS= read -r line; do
    case $line in
    '## Slices') in_slices=1 ;;
    '## '*) in_slices=0 ;;
    '### '*)
      if ((in_slices)); then
        n=$((n + 1))
        [[ $line =~ $slice_re ]] || echo "$plan: '$line' is not '### <n>. <title> — done|open'"
      else
        echo "$plan: '$line' sits outside '## Slices'; only slices are '###'"
      fi
      ;;
    esac
  done <"$plan"
  ((n)) || echo "$plan: no slices"
}

lint() {
  local out="" plan
  if [[ ${1:-} == --test ]]; then
    # A broken check could pass every plan, so it proves itself on fixtures first.
    for plan in scripts/fixtures/plans/pass-*.md; do
      out=$(lacks "$plan")
      [[ -z $out ]] || { echo "fixture should pass: $out"; return 1; }
    done
    for plan in scripts/fixtures/plans/fail-*.md; do
      [[ -n $(lacks "$plan") ]] || { echo "fixture should fail: $plan"; return 1; }
    done
    return 0
  fi
  for plan in $(plans); do
    out+=$(lacks "$plan")
    [[ -z $out || $out == *$'\n' ]] || out+=$'\n'
  done
  [[ -z $out ]] || { printf '%s' "$out"; return 1; }
}

case ${1:-} in
'') index ;;
--lint) lint "${2:-}" ;;
*)
  plan="$dir/$1.md"
  [[ -f $plan ]] || { echo "no plan $plan"; exit 1; }
  shift
  if (($# == 0)); then
    outline "$plan"
  else
    scripts/doc.sh "$plan" "$@"
  fi
  ;;
esac
