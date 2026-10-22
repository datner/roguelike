# Plan: tooling for working on the gui without opening it

From the gui cycle's postmortem. The agent can't run the window; it renders
screens headlessly (`egui_kittest` + wgpu) and reads the PNGs. Each image read
costs about width × height / 750 tokens, so the tools aim at reading less.

## Slices

### 4. Split the gui test module — done
`app/tests.rs` (the app's tests), `app/shots.rs` (the screenshot test),
`testing.rs` (setup and the headless window), so `app.rs` holds the app.

### 1. Cheaper screenshots — done
`scripts/shots.sh <dir> [shot…] [--crop map|side|bottom]`: render only the
named shots, cropped to a region taken from the layout constants.

### 2. Seeing animation
The harness runs on a clock the test sets. An `fx` shot renders an effect at
several moments and tiles the cropped frames into one contact sheet.

### 3. Visual regression
`scripts/shots.sh --check` compares shots with references in
`gui/tests/snapshots/` and writes a diff image on a mismatch; `--update`
refreshes them. Needs the GPU, so it stays out of `check.sh` and the flake.

### 5 + 6. Process and reading economy
`CLAUDE.md`: commit at slice ends and mark the slice done here; Edit for code,
Python only for one-offs; diagnostics before rebuilds; read little: locate
first (`rg -n -C3`, `map.sh --sigs`), then a bounded `Read` (the hooks enforce
this whatever the harness default says); test output through `grep`/`tail`;
read only the shots a change touches, cropped.
