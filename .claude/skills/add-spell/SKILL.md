---
name: add-spell
description: Recipe for magic in the game crate — a new spell, a new discipline (a way to pay for casting such as mana, heat, reagents or HP), or a new spell payload or shape (area, cone, summon). Use whenever a task touches `spell.rs`, casting, or a slice of `docs/plans/spells.md`.
---

# Spells and disciplines

Read ADR 0014 first. The seam it protects: a **spell** says what happens (`Spell { school, range, power }` + payload parts), a **discipline** on the caster says whether and at what price (`CheckCast` + `PayCost` handlers). `spell::cast` and the spell data stay resource-blind; every branch below ends with that still true.

Pick the branch; a task may take more than one.

## A. New spell (existing school and payloads)

1. `data/entities/<Name>.ron` modelled on `Firebolt.ron`. Its school must be claimed by a discipline some caster has, or the cast fails with "You have no way to cast …".
2. Teach it: attach it to the caster (the player's starting spells are in `GameState::new`; an NPC lists it in its blueprint).
3. Test: the caster can `ready_cast` it, and casting it on `bare_state()` does what its payload says.

## B. New discipline (a new way to pay)

1. **Parts** (the `add-part` skill): the rule on the caster (`<Resource>Casting { school, <price>_per_power }`) and, for a pool, the pool on the caster as `value` + `as_of` computed on read (no per-tick timer). Per-spell state (like `Recharging`) goes on the spell entity.
2. **Handlers** in `spell::register`, each with a named `prio.rs` constant:
   - `CheckCast`: for its own school only, set `claimed = true`; if it cannot pay, set `blocked` to the player-facing reason and `Break`.
   - `PayCost`: for its own school, push a `Command` that takes the price (the `add-command` skill for a new one). Handlers never mutate the world.
3. **Content:** give the discipline to a caster and a spell of its school.
4. **Tests:** claimed and paid; blocked with the exact reason and nothing spent; an unclaimed school is refused; save round-trip of a part-spent pool.
5. **Seam check:** `git diff` shows no change to `spell::cast`, `ready_cast` or `Spell`. If one was needed, stop and fix the seam (ADR 0014 update) before continuing.
6. Show the pool to the player through a `view.rs` function (the `add-term-ui` skill draws it).

## C. New payload or shape

1. Payload = a part on the spell (like `SpellDamage`); shape/targeting goes through a pure `area(..)` fn returning an ordered tile list (plan slice 3). Affected creatures are processed in `SpawnSeq` order, each emitting commands.
2. `cast` dispatches on the payload parts present; `aim` must preview through the same `area`/flight functions so the preview and the cast agree (ADR 0013).
3. Randomness comes from an existing stream that fits (`combat` for damage) or a new `rng::stream` id + `Rngs` field (ADR 0005); say in the report when draw order changes.
4. Tests first for the pure geometry (walls block, edges, ordering), then end to end through `Command::Cast`.

## Finish

ADR 0014 gets a paragraph when a decision changes; tick the slice off in `docs/plans/spells.md`; `scripts/check.sh` passes.
