#!/usr/bin/env bash
# PreToolUse(Bash) hook: steer file reading to Read/Grep/Glob. Blocks a
# pipeline that *starts* with a reader given a file operand; filters fed by a
# pipe, heredocs, `cat > f` and `sed -i` pass. Fails open on anything odd:
# a false block confuses the agent, a false allow costs nothing.
cmd=$(jq -r '.tool_input.command // empty' 2>/dev/null) || exit 0
set -f # words are data, never globs

# Heredoc bodies aren't commands; keep only the line that opens one.
[[ $cmd == *'<<'* ]] && cmd=${cmd%%$'\n'*}
cmd=${cmd//$'\\\n'/ }
# Quoted strings become one opaque word, so `rg "a|cat b"` doesn't split.
cmd=$(printf '%s' "$cmd" | sed -E "s/'[^']*'/Q/g; s/\"([^\"\\\\]|\\\\.)*\"/Q/g; s/[0-9]*>&[0-9-]*/ /g")
for op in '||' '&&' ';' '&' '(' ')' $'\n'; do cmd=${cmd//"$op"/$'\n;\n'}; done
cmd=${cmd//'|'/$'\n|\n'}

reads_file() {
  while [[ $1 == [A-Za-z_]*=* ]]; do shift; done
  local name=${1##*/} min n=0 a
  case $name in
    cat | head | tail | less | more | nl) min=1 ;;
    sed | awk) min=2 ;; # program + file
    *) return 1 ;;
  esac
  shift
  while (($#)); do
    a=$1
    shift
    case $a in
      '<<'*) [[ $a == '<<' || $a == '<<<' ]] && shift ;;
      '<'*) return 0 ;;
      '>' | '>>' | [0-9]'>' | [0-9]'>>') shift ;;
      '>'* | [0-9]'>'*) ;;
      -i* | --in-place*) [[ $name == sed ]] && return 1 ;;
      -e | -f) [[ $name == sed || $name == awk ]] && shift && min=1 ;;
      -n | -c) [[ $name == head || $name == tail ]] && shift ;;
      -v | -F) [[ $name == awk ]] && shift ;;
      -?*) ;;
      *) ((n++)) ;;
    esac
  done
  ((n >= min))
}

piped=0
while IFS= read -r seg; do
  case $seg in
    '|') piped=1 ;;
    ';') piped=0 ;;
    *) if ((!piped)) && reads_file $seg; then
      echo "Blocked: don't read files through Bash. Use Read (offset/limit for a range), Grep to search, Glob to find files; parallel calls are fine. Piping command output into head/tail is fine." >&2
      exit 2
    fi ;;
  esac
done <<<"$cmd"
exit 0
