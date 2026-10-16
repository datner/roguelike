# game crate — simulation

Pure library. No terminal/graphics crates, no I/O besides loading data files, no clock, no global RNG.

## Module map

- `cell.rs` — `Cell { glyph, fg, bg }`, `Rgb`: the only rendering output.
- `input.rs` — `InputEvent`, `Dir`, `PlayerAction`: the only input. Frontends translate raw keys into these and pass a `PlayerAction` to `take_turn`, which resolves the player entity and commands. Anything taking or returning a hecs type stays `pub(crate)`.
- `map.rs` — `Tile` (`Tile::is_open` is the one walk/see-through check: floor and exit), `Grid<T>` (i32 coords, `get` returns `Option`), spawn placement. `map::tests::room` is the hand-built 40×20 room tests use instead of the generator.
- `zone.rs` — zone size, `zone_seed` (SplitMix64 mixing of world seed and zone coords), the cellular-automata cave generator (`CaveParams` from `data/cave.ron`), `start_position`, `place_exit` (farthest floor from the start; no RNG). ADR 0009.
- `camera.rs` — `Camera`: which world tiles a viewport shows, and the only screen↔world coordinate conversion. ADR 0009.
- `entity.rs` — hecs components: plain data only (`Position`, `Appearance`, `Health`, `Armor`, `Attachments`, markers like `BlocksMovement`). `Attachments` = in use (parts fire); `Inventory` = carried, inert; `Body` = anatomy, `Equippable` = which part kind an item fits (ADR 0010).
- `blueprint.rs` — blueprints (RON, with `inherits` and `equipment`), loading and spawning. Inheritance: the child's field wins (per component variant; `equipment` absent inherits, present replaces, `[]` clears). Every referenced name is validated at load and resolved to a runtime-only `BlueprintId` (never serialized; persist names). `equipment` entries must fit the owner's `Body` (checked at load; `ResolvedBlueprint::equipped_on`).
- `command.rs` — `Command` enum and `apply_commands` (the only place the world is mutated during play). `GameState::run(command)` queues one command and applies it; use it unless several commands must share one drain. A command whose actor or target is `Dead` is skipped there, in one check; a new variant lists its entities in `Command::participants`.
- `event.rs` — typed, synchronous event dispatch to part handlers (`EventRegistry::on` / `fire`). Handlers get a `Ctx` with `&World`, the map, the RNG and an output `Vec<Command>` — they never mutate the world.
- `combat.rs` — damage events (`BeforeTakeDamage`, `Damaged`, `AfterHit` on the attacker for damaging melee hits), `strike` (wielded `Weapon` or natural `Melee`, plus the wielder's `Stats` modifier; ADR 0011), the armor (melee hits only; effect damage passes) and `OnHit` handlers, and `preview_damage` (dry run on a cloned RNG).
- `prio.rs` — every handler priority, in one file, so ordering is reviewable. New handlers get a named constant here, never an inline number. Priorities are unique per event type (`on` panics on a duplicate), so dispatch order never depends on registration order.
- `rng.rs` — `Rngs` (one `Pcg64` stream per subsystem) and the `stream` id constants.
- `grammar.rs` — `Subject`: subject–verb agreement for log messages.
- `state.rs` — `GameState`, `MessageLog`, `render_view(state, width, height)` (a viewport-sized grid through the camera; visible tiles in full, remembered terrain and items dimmed, the rest blank). `GameState::spawn`/`spawn_id`/`spawn_at` are the only way to create entities (clippy bans `World::spawn` in `game`); they assign `SpawnSeq` to the entity and its equipment (and `Energy` to anything with `Speed`). Tests build ad-hoc entities with `testkit` (below).
- `time.rs` — `Tick`, the `Timers` queue, and the energy scheduler (`step`, `advance`, `take_turn`). ADR 0006.
- `cost.rs` — every action's energy cost, and `cost::of`, which charges them in `apply_commands`.
- `effect.rs` — timed effects: `ApplyEffect`, `ModifyDuration` (`DurationChange`), expiry, bleeding ticks.
- `fov.rs` — symmetric shadowcasting (`compute`, `RADIUS`) and `update`, which refreshes `GameState.visible`/`seen`/`remembered_items` once per player turn. ADR 0007.
- `save.rs` — `to_ron` / `from_ron`: `GameState` to and from a versioned RON string (no file IO). Entities by value with references as save indices; a new component needs a `ComponentKind` and a `SavedComponent` variant, and a format change bumps `SAVE_VERSION`. ADR 0008.
- `view.rs` — read-only views for frontends (`inventory_view`, `equipment_view`, `character_sheet`, `ItemView`) and `ItemRef`, the item handle frontends pass back in `PlayerAction` (an item's `SpawnSeq`, so no hecs type crosses the seam).
- `equipment.rs` — body-part fitting (`free_part`), `equipped`, and the `Equip`/`Unequip` handlers with their refusal checks (`plan_equip`, `check_unequip`). Equipped = in `Attachments` with `EquippedOn(part)`. ADR 0010.
- `holding.rs` — every change to who holds what goes through here (save and event dispatch read the components directly): `attach`/`detach` (`Attachments`), `stow`/`unstow` (`Inventory`), `put_on`/`take_off` (equipped = attached + `EquippedOn`), and the `attached`/`carried` lists.
- `testkit.rs` (tests only) — the `Spawn` builder for ad-hoc entities: `thing("knife").item().at(3, 1).spawn(&mut state)`, with holders (`.carried_by`, `.equipped_on`, `.attached_to`). Registered through `GameState::spawn_built`, so they get `SpawnSeq`/`Energy` like any blueprint. Add a method when a component is used by more than one test; `.with(c)` covers the rest.
- `explore.rs` — `explore_step`: auto-explore as one player move per call (unfound `Item`s the player remembers, then frontier, then the exit; BFS over seen open tiles), halting with a log message when a hostile is visible or nothing is left. `ExploreItems` (on `GameState`, saved) picks what a step that finds items does: walk on, stop, or pick up; `cycle_explore_items` switches it. Items become `Found` via `Command::FindItems`, queued when the player moves.
- `ai.rs` — `decide`: a non-player actor's next `Command` (hostiles that see the player attack or step greedily toward them; others wait). All perception goes through `can_see_player` (ADR 0007).

Everything a frontend needs is re-exported from `lib.rs`; keep that list current.

## Patterns

**Reacting to something (a part's behaviour):** register a handler for (event, component) in the `EventRegistry` with a priority from `prio.rs`. It may rewrite the event, stop propagation, or push `Command`s into `ctx.out`. It must not mutate the world.

**Changing the world:** add a `Command` variant, handle it in `apply_commands` through a small `fn` taking `&mut GameState`. Handlers read what they need in a scoped block, drop the borrow, then write. If a handler wants to trigger follow-up effects, push more commands rather than recursing.

**hecs borrows:** a live `query` or `get` borrow blocks `get::<&mut _>` on the same component. Collect into a `Vec` (or compute a value) first, then mutate. Don't hold query results across `apply_commands`.

**Randomness:** take `&mut Pcg64` as a parameter and pass your subsystem's stream (`&mut state.rngs.combat`, `.spawn`, `.map`). Never draw from another subsystem's stream; a new subsystem gets a new id in `rng::stream` and a new `Rngs` field (ADR 0005). Never create a new RNG mid-simulation. Order of RNG calls within a stream is part of determinism — changing it changes seeded worlds for that subsystem, so mention it when you do.

**Log messages about an entity:** build them with `grammar::Subject::of(world, entity)` — `sentence("take", "3 damage")`, `possessive()`, `verb(..)` — so the player reads "You take…" and others "The goblin takes…". `Name` holds the bare noun ("goblin"); `Subject` adds "the".

**Time:** anything timed stores `start` + `duration` and derives its end; ask `Timed::is_active(now)`, never a timer. Change durations only with `Command::ModifyDuration`. Timers are idempotent checks, so a new `Timer` variant's applier must re-validate and no-op when stale (ADR 0006). A new player/AI action gets a cost in `cost.rs`.

**Collections:** `BTreeMap`/`BTreeSet` or `Vec`. If a `HashMap` is truly needed and never iterated, use `#[expect(clippy::disallowed_types, reason = "...")]` with an honest reason.

## Tests

The standard shape — build a state from a seed, queue commands, apply, assert:

```rust
let mut state = bare_state(); // state::tests: the hand-built room, player at (1, 1) only
state.run(Command::TryMove(state.player, Dir::E)); // queue + apply_commands
assert_eq!(*state.world.get::<&Position>(state.player).unwrap(), Position { x: 2, y: 1 });
```

- Every new `Command` gets tests for its success path and each blocked/failure path (including the log message).
- New generation code gets a same-seed-same-result test. Tests not about generation use `bare_state` / `map::tests::room`, never a generated zone.
- Build blueprints inline in tests unless the test is specifically about loading `data/`.
