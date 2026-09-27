---
name: add-part
description: Recipe for adding data-driven content to the game — a new RON blueprint, or a new reusable part (component + event handlers) that blueprints can use. Use when a task adds a creature, item, terrain feature, or a new property things can have.
---

# Adding content and parts

Content lives in `data/**/*.ron`; behaviour lives in reusable parts (ADR 0004). Prefer a new RON file over new Rust. Add Rust only when no existing part expresses the behaviour.

## Only new content (existing parts suffice)

1. Add `data/entities/<PascalName>.ron`. Look at an existing file with a similar role first (`Creature.ron`, `Goblin.ron`, `LeatherArmor.ron`) and use `inherits:` instead of repeating a parent's components.
2. Make sure the load-every-blueprint test covers it (add that test if it doesn't exist yet).
3. `scripts/check.sh`.

## A new part (new property or behaviour)

1. **Component** in `game/src/entity.rs`: small plain data deriving `Debug, Clone, PartialEq, Serialize, Deserialize` (plus `Copy`/`Eq` when possible). Use a unit struct for pure flags (like `BlocksMovement`). No methods holding game logic.
2. **Make it listable in RON:** register it wherever `blueprint.rs` maps component names to types. Follow what an existing component such as `Health` does there, rather than inventing a new mechanism.
3. **Behaviour** = event handlers registered for (event, your component), with a new named priority constant in `game/src/prio.rs`. Handlers read `ctx.world`, may rewrite or stop the event, and push `Command`s into `ctx.out`. If you need a new kind of state change, use the `add-command` skill.
4. **Data:** add or update RON files that use the part.
5. **Tests:** the RON parses; spawning attaches the component (including via `inherits` and `equipment` if relevant); the behaviour works end to end through `fire` + `apply_commands` with a fixed seed.
6. `scripts/check.sh`.

Don't add a generic "properties" map, scripting, or a new dispatch mechanism unless the task explicitly asks for it. Those need an ADR first.
