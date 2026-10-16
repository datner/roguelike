# game crate — simulation

Pure library. No terminal/graphics crates, no I/O besides loading data files, no clock, no global RNG.

## Module rules

What each module is for is in the index (`docs/MAP.md`); its API is
`scripts/map.sh --sigs <path>`. This list keeps only the rules the code
can't show at a glance.

- `input.rs`/`view.rs` — the seam: `term` passes a `PlayerAction` and reads plain views. Anything taking or returning a hecs type stays `pub(crate)`; items cross as `ItemRef` (their `SpawnSeq`).
- `map.rs` — `Tile::is_open` is the one walk/see-through check. Out-of-bounds `Grid::get` is `None`: treat it as wall. Tests use `map::tests::room` (hand-built 40×20), never the generator.
- `zone.rs` — `place_exit` uses no RNG; changing the generator's draw order or `data/cave.ron` changes every seeded world (ADR 0009).
- `entity.rs` — plain data only. `Attachments` = in use (parts fire); `Inventory` = carried, inert; equipped = attached + `EquippedOn` (ADR 0010).
- `holding.rs` — every change to `Attachments`, `Inventory` or `EquippedOn` goes through here. Save and event dispatch read them directly.
- `blueprint.rs` — inheritance: the child's field wins (per component variant; `equipment` and `spells` absent inherit, present replace, `[]` clears). Names are validated at load and resolved to runtime-only `BlueprintId`s: never serialize one, persist names. `equipment` must fit the owner's `Body` (checked at load).
- `spawn.rs`/`state.rs` — `GameState::spawn(Spawn)` is the only way to create entities (clippy bans `World::spawn`); it assigns `SpawnSeq` (and `Energy` to anything with `Speed`). Describe placement and holder on the `Spawn` rather than inserting them afterwards.
- `command.rs` — the only place the world is mutated during play. `GameState::run(command)` queues and applies one command; push several and apply once only when they must share a drain. Commands involving a `Dead` entity are skipped in one check; a new variant lists its entities in `Command::participants`.
- `event.rs` — handlers get `&World`, the map, an RNG and an output `Vec<Command>`; they never mutate the world.
- `prio.rs` — every handler priority, as a named constant (never inline). Unique per event type (`on` panics on a duplicate).
- `combat.rs` — armor resists typed (melee) damage per kind, summed over worn pieces; untyped effect damage passes (ADR 0012). Damage previews run on a cloned combat stream.
- `fov.rs` — refreshed once per player turn, never per actor; AI sight reuses it (ADR 0007).
- `save.rs` — a new component needs a `ComponentKind` and a `SavedComponent` variant (remap any `Entity`/`BlueprintId` both ways); any format change bumps `SAVE_VERSION` (ADR 0008).
- `explore.rs` — judges only from what the player knows (seen tiles, remembered items). `ExploreItems` is game state and saved.
- `ammo.rs` — a nocked arrow is the launcher's own `Inventory`, a quiver's arrows are the quiver's: never add a new holder list for them. `Fire` takes the nocked arrow first, then an *equipped* quiver.
- `ranged.rs` — `Trail`s are animation-only: `GameState.trails` is never read by the simulation or saved.
- `spell.rs` — a cast runs `ModifyCast` (caster), `CheckCast` (spell, then caster), `CastRisk`, `PayCost` (whoever claimed); resources, their rules and the stat a spell scales with belong to discipline parts (`CooldownCasting`, `ManaCasting`, `BloodCasting`, a `Scroll`), never to `cast` or the spell data; pools are `value` + `as_of` computed on read. Payloads (`SpellDamage`, `Area`, `Chain`, `Summon`, `Delay`) are read in `footprint`/`cast` only, and `footprint` is shared with `aim` and `ai::spell_at` (ADR 0014).
- `area.rs` — pure geometry: what a shape covers is decided here and nowhere else; `spell::footprint` is the one caller, for both the cast and `aim`, so the preview and the cast agree.
- `ranged.rs`/`aim.rs` — flight is `ranged::flight` (a line, first creature or wall wins); `aim` previews through the same functions the command uses, so keep them shared (ADR 0013). Missile damage is untyped by `AfterHit`: only melee fires it.
- `ai.rs` — perception of the player goes through `can_see_player` (ADR 0007), of anyone else through `perceives` (both in the player's view, ADR 0015); never compute a view per actor.
- `side.rs` — who is an enemy is `side::enemies`, never a check for `Hostile`: summons take their owner's side (ADR 0015).
- `testkit.rs` — test-only methods on `Spawn` (`thing("knife").item().at(3, 1).make(&mut state)`) and `last_log`. Add a shortcut when a component is used by more than one test; `.with(c)` covers the rest.

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
