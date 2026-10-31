#!/usr/bin/env bash
# Runs the game's tests on a copy of `data/` with its balance numbers moved (health, damage, armor,
# skill levels, spawn counts), so a test that pins what the content says fails here even though it
# passes on the real data (docs/plans/content-free-tests.md). Tests reach the copy through
# `testkit::data()`, which reads ROGUELIKE_TEST_DATA.
# Usage: nudge.sh [cargo test args...]   Quiet on success; the failing tests' names on failure.
set -euo pipefail
cd "$(dirname "$0")/.."

copy=$(mktemp -d)
trap 'rm -rf "$copy"' EXIT
cp -R data/. "$copy"

# Each a small move up, as a balance pass makes: big enough to change any literal a test copied.
# Whole files at once (-0), so a `Skills({ ... })` spread over lines moves too.
# shellcheck disable=SC2016 # Perl, not shell.
nudge='
  s#Health\(current: (\d+), max: (\d+)\)#"Health(current: ".($1+3).", max: ".($2+3).")"#ge;
  s#\b(Melee|Weapon|SpellDamage|Throwable)\(min: (\d+), max: (\d+)#"$1(min: ".($2+1).", max: ".($3+1)#ge;
  s#(Armor\([^)]*\))#my $a = $1; $a =~ s/: (\d+)/": ".($1+1)/ge; $a#ge;
  s#(Skills\(\x7b[^\x7d]*\x7d\))#my $s = $1; $s =~ s/": (\d+)/"\": ".($1+1)/ge; $s#ge;
  s#\bcount: (\d+)#"count: ".($1+1)#ge;
'
fd -e ron . "$copy" -x perl -0pi -e "$nudge"

out=$(ROGUELIKE_TEST_DATA=$copy cargo test -p game --quiet "$@" 2>&1) && exit 0
# A test binary that died (a panic in a build, a stack overflow) lists no failures: show why.
if ! rg -q '^test result: FAILED' <<<"$out"; then
  tail -n 20 <<<"$out"
  exit 1
fi
echo "$out" | rg '^test .* FAILED$|^    \S+$' | rg -o '\S+::\S+' | sort -u
echo "These tests pin content: give them values of their own, or compare with the data they load."
exit 1
