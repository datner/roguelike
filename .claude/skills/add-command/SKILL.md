---
name: add-command
description: Recipe for adding a new player or world action (move, attack, pick up, open, etc.) to the game crate as a Command. Use whenever a task adds or changes something that mutates game state.
---

# Adding a Command

Every state change goes through `game::Command` and `apply_commands` (ADR 0002). Follow these steps in order.

1. **Variant.** Add a variant to `Command` in `game/src/command.rs`. Carry `Entity` ids and plain `Copy` data only — no references, no `String`s unless unavoidable.
2. **Handler.** Add a `fn verb_noun(state: &mut GameState, …)` next to `try_move` and dispatch to it from the `match` in `apply_commands`. Inside the handler:
   - Read everything you need in a scoped block or into locals, so every hecs borrow is dropped before you write.
   - On failure, push a player-facing message to `state.log` and `return`. Don't panic.
   - Need randomness? Use `&mut state.rng`. Never create an RNG.
   - Need other parts to react (armor reducing damage, etc.)? Fire an event through the `EventRegistry` and apply the commands its handlers emit, instead of hardcoding the reaction here.
   - Need a follow-up state change (damage → death)? `state.commands.push_back(...)` instead of calling the other handler directly.
   - Is it an *action* an actor takes (it should use up their turn)? Give it a cost constant in `game/src/cost.rs` and a `cost::of` arm; `apply_commands` charges it. Consequences of an action (attack from a bump, damage) cost nothing.
   - Anything timed follows ADR 0006: store `start` + `duration`, schedule a `Timer` whose applier re-checks timestamps and no-ops when stale.
3. **Input (only if the player triggers it).** Add an `InputEvent` variant in `game/src/input.rs`, bind a key in `term/src/input.rs::map_key_event`, and translate it in `term/src/app.rs` into `game::take_turn(state, command)`. Update README "Controls".
4. **Re-export** anything new that `term` needs from `game/src/lib.rs`.
5. **Tests** in `game/src/state.rs` (or the command's module): one for the success path, one per failure path asserting both the unchanged state and the exact log message. Use a fixed seed.
6. **Verify:** `scripts/check.sh`.

Don't add a new dispatch mechanism, or a second scheduler or clock, as part of this unless the task asks for one. Note it as a follow-up instead.
