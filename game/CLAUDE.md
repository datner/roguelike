# game crate — simulation

Pure library. No terminal/graphics crates, no I/O besides loading data files, no clock, no global RNG.

## Module map

- `cell.rs` — `Cell { glyph, fg, bg }`, `Rgb`: the only rendering output.
- `input.rs` — `InputEvent`, `Dir`: the only input. Frontends translate raw keys into these.
- `map.rs` — `Tile`, `Grid<T>` (i32 coords, `get` returns `Option`), map generation.
- `entity.rs` — hecs components: plain data only (`Position`, `Appearance`, `Health`, `Armor`, `Attachments`, markers like `BlocksMovement`).
- `blueprint.rs` — blueprints (RON, with `inherits` and `equipment`), loading and spawning. Inheritance: the child's field wins (per component variant; `equipment` absent inherits, present replaces, `[]` clears).
- `command.rs` — `Command` enum and `apply_commands` (the only place the world is mutated during play).
- `event.rs` — typed, synchronous event dispatch to part handlers (`EventRegistry::on` / `fire`). Handlers get a `Ctx` with `&World`, the map, the RNG and an output `Vec<Command>` — they never mutate the world.
- `combat.rs` — damage events (`BeforeTakeDamage`, `Damaged`), the armor handler, and `preview_damage` (dry run on a cloned RNG).
- `prio.rs` — every handler priority, in one file, so ordering is reviewable. New handlers get a named constant here, never an inline number. Priorities are unique per event type (`on` panics on a duplicate), so dispatch order never depends on registration order.
- `rng.rs` — `Rngs` (one `Pcg64` stream per subsystem) and the `stream` id constants.
- `grammar.rs` — `Subject`: subject–verb agreement for log messages.
- `state.rs` — `GameState`, `MessageLog`, `render_view`. `GameState::spawn`/`spawn_at` assign `SpawnSeq` (and `Energy` for anything with `Speed`); use them during play instead of `Blueprints::spawn`.
- `time.rs` — `Tick`, the `Timers` queue, and the energy scheduler (`step`, `advance`, `take_turn`). ADR 0006.
- `cost.rs` — every action's energy cost, and `cost::of`, which charges them in `apply_commands`.
- `effect.rs` — timed effects: `ApplyEffect`, `ModifyDuration` (`DurationChange`), expiry, bleeding ticks.
- `ai.rs` — `decide`: a non-player actor's next `Command` (hostiles attack or step greedily toward the player).

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
let mut state = bare_state(); // state::tests: GameState::bare(1, test_blueprints()), player only
state.commands.push_back(Command::TryMove(state.player, Dir::E));
apply_commands(&mut state);
assert_eq!(*state.world.get::<&Position>(state.player).unwrap(), Position { x: 2, y: 1 });
```

- Every new `Command` gets tests for its success path and each blocked/failure path (including the log message).
- New generation code gets a same-seed-same-result test.
- Build blueprints inline in tests unless the test is specifically about loading `data/`.
