#!/usr/bin/env bash
# `fixtures/data/` is the content `front` and `gui` tests and the shots run on: a copy of `data/`
# taken when the data's format last changed, so balance and new content never move a flow test or a
# screenshot (docs/plans/content-free-tests.md). Refresh it only when the copy no longer loads (a
# renamed or required field), then run `scripts/shots.sh --check` and update what moved, in one commit
# with the format change.
# Usage: fixtures.sh   Copies data/ over fixtures/data/.
set -euo pipefail
cd "$(dirname "$0")/.."

rm -rf fixtures/data
mkdir -p fixtures
cp -R data fixtures/data
echo "fixtures/data refreshed from data/: run scripts/shots.sh --check and the front and gui tests"
