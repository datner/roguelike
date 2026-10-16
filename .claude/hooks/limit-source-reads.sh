#!/usr/bin/env bash
# .claude/hooks/limit-source-reads.sh
# PreToolUse(Read): block whole-file reads of large Rust files; cap window size.
input=$(cat) || exit 0
command -v jq >/dev/null || exit 0

path=$(jq -r '.tool_input.file_path // empty' <<<"$input")
limit=$(jq -r '.tool_input.limit // empty' <<<"$input")
[[ -n "$path" && -f "$path" && "$path" == *.rs ]] || exit 0

max=150
lines=$(wc -l <"$path" | tr -d ' ')

if [[ -z "$limit" && "$lines" -gt "$max" ]] || [[ -n "$limit" && "$limit" -gt "$max" ]]; then
  echo "Blocked: $path is $lines lines. Find the symbol with Grep (-n) or scripts/map.sh --sigs $path, then Read with offset and limit <= $max around it." >&2
  exit 2
fi
exit 0
