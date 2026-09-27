#!/usr/bin/env bash
# PostToolUse hook: format a .rs file right after Claude edits it, so style
# never costs a review round or a failed check. Silent and never blocking.
input=$(cat)
file=$(printf '%s' "$input" | sed -n 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\.rs\)".*/\1/p' | head -n1)
[[ -z "$file" || ! -f "$file" ]] && exit 0
command -v rustfmt >/dev/null 2>&1 || exit 0
rustfmt --edition 2024 "$file" >/dev/null 2>&1 || true
exit 0
