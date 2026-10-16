---
name: add-part
description: Recipe for adding data-driven content to the game — a new RON blueprint, or a new reusable part (component + event handlers) that blueprints can use. Use when a task adds a creature, item, terrain feature, or a new property things can have.
---

# Adding content and parts

Content lives in `data/entities/*.ron`; behaviour lives in reusable parts (ADR 0004). Reach for a new RON file first, and add Rust only when no existing part expresses the behaviour.

## Only new content (existing parts suffice)

1. Add `data/entities/<PascalName>.ron`, modelled on a file with a similar role, using `inherits:` for whatever a parent already gives.
2. `blueprint::tests::loads_the_data_directory` covers it; if a test needs it by name, add an entry to `state::tests::test_blueprints()`.
3. `scripts/check.sh` passes.

## A new part (new property or behaviour)

A part is registered in four places; it is done when all four exist and the round-trip test passes.

1. **Component** in `game/src/entity.rs`: plain data deriving `Debug, Clone, PartialEq, Serialize, Deserialize` (plus `Copy`/`Eq` when possible); a unit struct for a pure flag. Timed state is a `Timed` (ADR 0006); a regenerating pool stores `value` + `as_of` and computes on read.
2. **Blueprint:** a `ComponentDef` variant and its `builder.add` arm in `blueprint.rs`.
3. **Save:** a `ComponentKind` (also in its `ALL` list) and a `SavedComponent` variant with both arms in `save.rs`, remapping any `Entity`/`BlueprintId` in both directions. Bump `SAVE_VERSION` and add a `…_survive(s)_a_round_trip` test.
4. **Behaviour:** handlers for (event, component) with a new named constant in `prio.rs`. They read `ctx.world` and push `Command`s into `ctx.out`; a new kind of state change goes through the `add-command` skill. Pure data read by an existing system needs no handler.

Then:

5. **Data:** add or update the RON files that use the part.
6. **Tests:** the RON parses; spawning attaches the component (through `inherits` and `equipment` where relevant); the behaviour works end to end through `fire` + `apply_commands` on `bare_state()`. If more than one test builds the part ad hoc, add a `testkit` shortcut.
7. If `term` should show it, expose it through a plain view in `view.rs` (the `add-term-ui` skill draws it).
8. `scripts/check.sh` passes.

A generic "properties" map, scripting, or a new dispatch mechanism needs an ADR first: propose it instead of building it.
