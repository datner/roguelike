#!/usr/bin/env bash
# Project skills (.claude/skills/) are recipes that name files, functions and other skills; a rename
# leaves them pointing at nothing, and an agent follows a dead pointer without noticing. This checks
# every name a skill puts in backticks still exists.
# Usage: skills.sh   Quiet on success; names each skill and the dead name on failure.
set -euo pipefail
cd "$(dirname "$0")/.."

code=(game/src front/src gui/src bench/src)
out=""
for skill in .claude/skills/*/SKILL.md; do
  while IFS= read -r name; do
    case $name in
    *[\<\*]*) ;; # A placeholder or a glob (`data/entities/<Name>.ron`) names no one file.
    */*.*)
      # A path: from the root, or a module path like `app/shots.rs` inside a crate.
      [[ -e $name ]] || fd -q -p "/$name\$" "${code[@]}" data docs scripts || out+="$skill: no file '$name'"$'\n'
      ;;
    *.rs | *.ron | *.md | *.sh)
      fd -q -F "$name" "${code[@]}" data docs scripts || out+="$skill: no file '$name'"$'\n'
      ;;
    [a-z]*::[a-zA-Z_]* | [A-Z]*::[a-zA-Z_]*)
      # `module::item` or `Type::item`: the item must be defined somewhere in the workspace code.
      item=${name##*::}
      item=${item%%[!a-zA-Z0-9_]*}
      rg -q -w "(fn|struct|enum|trait|type|const|mod) $item|$item[,(]? *(\{|=>|\(|,)|$item$" "${code[@]}" ||
        out+="$skill: nothing defines '$name'"$'\n'
      ;;
    esac
  done < <(rg -o --no-filename '`([^` ]+)`' -r '$1' "$skill" | sort -u)
  # "the `x` skill" names another project skill.
  while IFS= read -r other; do
    [[ -d .claude/skills/$other ]] || out+="$skill: no skill '$other'"$'\n'
  done < <(rg -o --no-filename '`([a-z-]+)` skill' -r '$1' "$skill" | sort -u)
done
[[ -z $out ]] || { printf '%s' "$out"; exit 1; }
