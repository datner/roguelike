# front crate — what every frontend shares

The keyboard flows, the title menu and save files, shared by frontends (today, `gui`)
(ADR 0020). **No drawing and no key bindings here**, and no game rules: if
you're about to decide what a move/attack/pickup *does*, it belongs in `game`
as a `Command`.

## Module rules

What each module is for is in the index (`docs/MAP.md`); its API is
`scripts/map.sh --sigs <path>`.

- `session.rs` — the one place a frontend's input becomes a turn or a mode change. A flow rule (a refused shot keeps the cursor up, an uncastable spell keeps the list up with the reason logged, any key stops a run) lives here once; never in a frontend.
- `menu.rs`, `overlay.rs`, `targeting.rs` — pure state: `handle` takes input (plus fresh `game` views) and returns an action; `Session` or `title` performs it. Frontends draw them from accessors and `view`s, never from private fields.
- `targeting.rs` — what a shot does is `game::aim`, what a spell costs is `game::spells`, what looking reveals is `game::inspect` (never name a resource or a rule here).
- `saves.rs` — rename never overwrites; a save that failed to load is never overwritten; no save after death. ADR 0008.
- `title.rs` — the only file IO besides `saves.rs`; messages it returns are shown as-is by every frontend.
- Text the player reads (status lines, row text, hints) is built here, so the frontends say the same thing; styling (dim, colour, reversed) is the frontend's.

## Rules

- No ratatui, crossterm, egui or other drawing crate; never refer to hecs; never name `Command` or `Entity` (all checked by `scripts/check.sh`).
- Timing is the frontend's: `Session::auto_step` is one turn of a run, the caller paces it. No clock here.
- Test flows by driving a `Session` with `Input`s (`session.rs` tests); `crate::tests::state()` is a seed-3 game from the real data.
