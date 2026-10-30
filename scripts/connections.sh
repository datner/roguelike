#!/usr/bin/env bash
# The interconnection rule (docs/plans/README.md): every game-system plan names, under
# `## Connections`, at least two systems it reads and two that read it; tool plans carry `Exempt:`.
# Usage: connections.sh [--test] [plan.md...]   (default: every docs/plans/*.md but the README)
# Quiet on success; names each plan and what it lacks on failure.
set -euo pipefail
cd "$(dirname "$0")/.."

readme=docs/plans/README.md

# The systems a connection may name: the README's `Systems:` line, comma-separated.
systems=$(rg -m1 -o '^Systems: (.*)$' -r '$1' "$readme" | tr -d ' ')
if [[ -z "$systems" ]]; then
  echo "$readme has no 'Systems:' line"
  exit 1
fi

# Prints what one plan lacks, nothing if it passes.
lacks() {
  local plan=$1
  if rg -q '^Exempt: \S' "$plan"; then
    return
  fi
  if ! rg -q '^## Connections$' "$plan"; then
    echo "$plan: no '## Connections' section and no 'Exempt:' line"
    return
  fi
  # shortcut: a provisional section passes until interconnection.md's slice 4 makes the check strict.
  if rg -q '^\*TODO\*' "$plan"; then
    return
  fi
  local section=none in_conn=0 line sys
  local -A reads=() read_by=() unknown=()
  while IFS= read -r line; do
    if [[ $line == '## Connections' ]]; then
      in_conn=1
      continue
    fi
    [[ $in_conn == 1 ]] || continue
    [[ $line == '## '* ]] && break
    case $line in
    '**Reads**'*) section=reads ;;
    '**Read by**'*) section=read_by ;;
    '- '*)
      # A vision-only connection is a direction, not a connection yet.
      [[ $line == *'*(vision)*'* ]] && continue
      sys=${line#- }
      sys=${sys%%:*}
      sys=${sys//\*/}
      if [[ ",$systems," != *",$sys,"* ]]; then
        unknown[$sys]=1
        continue
      fi
      case $section in
      reads) reads[$sys]=1 ;;
      read_by) read_by[$sys]=1 ;;
      esac
      ;;
    esac
  done <"$plan"
  local name
  for name in "${!unknown[@]}"; do
    echo "$plan: '$name' is not in the systems list ($readme)"
  done
  if ((${#reads[@]} < 2)); then
    echo "$plan: Reads names ${#reads[@]} system(s), needs 2"
  fi
  if ((${#read_by[@]} < 2)); then
    echo "$plan: Read by names ${#read_by[@]} system(s), needs 2"
  fi
}

# The check checks itself on fixtures, so a broken parser can't pass every plan.
if [[ ${1:-} == --test ]]; then
  dir=scripts/fixtures/connections
  for good in "$dir"/pass-*.md; do
    out=$(lacks "$good")
    [[ -z $out ]] || {
      echo "fixture should pass: $out"
      exit 1
    }
  done
  for bad in "$dir"/fail-*.md; do
    [[ -n $(lacks "$bad") ]] || {
      echo "fixture should fail: $bad"
      exit 1
    }
  done
  exit 0
fi

if (($# == 0)); then
  for plan in docs/plans/*.md; do
    [[ $plan == "$readme" ]] || set -- "$@" "$plan"
  done
fi
out=""
for plan in "$@"; do
  out+=$(lacks "$plan")
  [[ -z $out || $out == *$'\n' ]] || out+=$'\n'
done
if [[ -n $out ]]; then
  printf '%s' "$out"
  exit 1
fi
