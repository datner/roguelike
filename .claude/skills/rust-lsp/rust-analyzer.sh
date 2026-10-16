#!/usr/bin/env bash
# Starts rust-analyzer from this project's flake devShell, so it is the one
# pinned by rust-toolchain.toml, never one from the global PATH. stdout is
# the LSP channel: nothing else may write to it.
set -euo pipefail

root=${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "$0")/../../.." && pwd)}

# direnv reuses the cached `use flake` environment, so startup is fast.
if command -v direnv >/dev/null && direnv exec "$root" true >/dev/null 2>&1; then
  exec direnv exec "$root" rust-analyzer "$@"
fi
exec nix develop "$root" --command rust-analyzer "$@"
