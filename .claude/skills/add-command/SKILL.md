---
name: add-command
description: Recipe for adding a new player or world action (move, attack, pick up, open, etc.) to the game crate as a Command. Use whenever a task adds or changes something that mutates game state.
---

# Adding a Command

Every state change goes through `game::Command` and `apply_commands` (ADR 0002). Follow these steps in order; `game/CLAUDE.md` holds the rules each step relies on (borrows, RNG streams, time, log grammar).

1. **Variant.** Add it to `Command` in `game/src/command.rs`, carrying `Entity` ids and plain `Copy` data. List its entities in `Command::participants` so commands on a `Dead` entity are skipped. Done when `participants` and the `match` in `apply_commands` both compile with the new arm.
2. **Handler.** A small `fn` in the command's own module (`spell::cast`, `ranged::throw`, …) or in `command.rs`, dispatched from `apply_commands`.
   - Read into locals first, then write.
   - Failure: log a player-facing message and return.
   - Reactions from other parts (armor, disciplines): fire an event and apply the commands its handlers emit.
   - Follow-ups (damage → death): `state.commands.push_back(..)`.
3. **Checks shared with previews.** If the action can be refused, write one `check_*`/`ready_*` fn returning `Result<_, String>` and call it from the handler, from `reject_input` (so refused input costs no time), and from any `aim`/`view` preview. Done when no refusal message is written in two places.
4. **Cost.** An action an actor chooses gets a constant in `cost.rs` and a `cost::of` arm. Consequences (damage from a hit) cost nothing.
5. **Player input (only if the player triggers it).** Add a `PlayerAction` variant in `game/src/input.rs` (items cross the seam as `ItemRef`) and translate it to the command in `time::take_turn`, resolving each `ItemRef` against the list the player may use. The key and any chooser/aim UI is the `add-ui` skill.
6. **Re-export** from `game/src/lib.rs` what `front` and `gui` need.
7. **Tests** next to the handler: the success path, and each refusal asserting unchanged state and the exact log line (`testkit::last_log`). Use `bare_state()` and `state.run(command)`. A player action also gets one test through `take_turn`.
8. `scripts/check.sh` passes.

A new dispatch mechanism, scheduler or clock is out of scope for this recipe: note it as a follow-up.
