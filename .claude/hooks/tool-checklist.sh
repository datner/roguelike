#!/usr/bin/env bash
# SessionStart: puts the start-of-task tool checklist into the session's
# context (on startup, resume, clear and compaction alike), because a line in
# CLAUDE.md and a memory were both not enough to keep the agent using its tools.
set -euo pipefail

read -r -d '' checklist <<'EOF' || true
Tool checklist for this project (run it at the start of each task, and again after compaction or a background job):
1. IDE: check the mcp__ide__* tools are there; if not, say so.
2. LSP: load it (ToolSearch "select:LSP") and use goToDefinition / findReferences / incomingCalls for symbols. Never rg for a symbol.
3. Compile errors come from IDE diagnostics; build only to run tests, the bench or shots.
4. A module's API: scripts/map.sh --sigs <path>, before opening the file.
5. Text search: the Grep tool. Reading: Read with offset/limit. Never cat/sed/head on files.
6. Changes: Edit/Write (the rustfmt hook runs). Repeated multi-site edits mean the design needs fixing or a tool, not a script.
EOF

jq -n --arg context "$checklist" \
  '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $context}}'
