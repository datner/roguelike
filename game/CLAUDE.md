# game crate — simulation

Pure library. No terminal/graphics crates, no I/O besides loading data files, no clock, no global RNG.

## Module rules

What each module is for is in the index (`docs/MAP.md`); its API is
`scripts/map.sh --sigs <path>`. This list keeps only the rules the code
can't show at a glance.

- `input.rs`/`view.rs` — the seam: frontends pass a `PlayerAction` and reads plain views. Anything taking or returning a hecs type stays `pub(crate)`; items cross as `ItemRef` (their `SpawnSeq`).
- `map.rs` — `Tile::is_open` is the one walk/see-through check. Out-of-bounds `Grid::get` is `None`: treat it as wall. Tests use `map::tests::room` (hand-built 40×20), never the generator.
- `zone.rs` — `place_stairs_down` uses no RNG; changing the generator's draw order or a dungeon's cave tuning changes every seeded world (ADR 0009).
- `dive.rs` — a floor exists only while the player is on it; `dive::build` is the one way to make one (new game and stairs alike), its seed from `floor_seed`. Only the player, what they hold and their summons cross floors.
- `camp.rs` — camp is data (`data/camp.ron`); a new game starts there when content has one (tests built from `test_blueprints` have none, so start on a floor). Nothing hostile spawns there. Items left on its floor are `Stashed` (no `Position`) while the player is away, so a floor never sees them; `dive::enter` swaps them out and back.
- `phase.rs` — phases are data (`data/phases.ron`, first = where the world starts); saved by key, passed as an index. Waiting is camp-only (`wait_refusal`, shared with the frontends); a dive keeps its phase in `Place::Floor`, so it never changes mid-dive.
- `origin.rs` — origins are data (`data/origins.ron`, first = the default); an origin's blueprint inherits the bare `Player`, so kit, skills and techniques belong to origins, never to `Player`. `spawn_player` is the one way a player is made; it draws no RNG (ADR 0025).
- `dungeon.rs` — dungeons are data (`data/dungeons/<Key>.ron`), checked against the blueprints at load; spawn counts belong there, never in code. A dungeon picks its layout with `Generator` (`Cave` in `zone.rs`, `Rooms` in `rooms.rs`); a new generator is a variant there.
- `legacy.rs` — a `Record` holds names only (`Made`, skill names, keys), never ids or entities, so it outlives content changes; readers skip a name that no longer exists (ADR 0026). Ruins are built once, by `build_ruins` from `GameState::new_in_lineage`, on a local `LEGACY` stream: never draw from it anywhere else.
- `entity.rs` — plain data only. `Attachments` = in use (parts fire); `Inventory` = carried, inert; equipped = attached + `EquippedOn` (ADR 0010).
- `holding.rs` — every change to `Attachments`, `Inventory` or `EquippedOn` goes through here. Save and event dispatch read them directly.
- `blueprint.rs` — inheritance: the child's field wins (per component variant; `equipment` and `spells` absent inherit, present replace, `[]` clears). Names are validated at load and resolved to runtime-only `BlueprintId`s: never serialize one, persist names. `equipment` must fit the owner's `Body` (checked at load).
- `spawn.rs`/`state.rs` — `GameState::spawn(Spawn)` is the only way to create entities (clippy bans `World::spawn`); it assigns `SpawnSeq` (and `Energy` to anything with `Speed`). Describe placement and holder on the `Spawn` rather than inserting them afterwards.
- `command.rs` — the only place the world is mutated during play. `GameState::run(command)` queues and applies one command; push several and apply once only when they must share a drain. Commands involving a `Dead` entity are skipped in one check; a new variant lists its entities in `Command::participants`.
- `event.rs` — handlers get `&World`, the map, an RNG and an output `Vec<Command>`; they never mutate the world.
- `prio.rs` — every handler priority, as a named constant (never inline). Unique per event type (`on` panics on a duplicate).
- `combat.rs` — armor resists typed (melee) damage per kind, summed over worn pieces; untyped effect damage passes (ADR 0012). Damage previews run on a cloned combat stream. A blow resolves through `combat::odds` (the `AttackOdds` event, side-effect free) then `combat::resolve` (ADR 0017); tests about what a hit does use `testkit::sure_hits`.
- `fov.rs` — refreshed once per player turn, never per actor; AI sight reuses it (ADR 0007).
- `save.rs` — a new component goes on one list in `components!` (`plain`, `markers`, or `custom` with a `CustomSaved` variant and its arms in `save_custom`/`load_custom`, remapping any `Entity`/`BlueprintId` both ways); any format change bumps `SAVE_VERSION` (ADR 0008).
- `blueprint.rs` — a component a blueprint can declare goes on one list in `component_defs!` (`plain`, `markers`, or `resolved` when it holds names spawned from `ResolvedBlueprint`).
- `explore.rs` — judges only from what the player knows (seen tiles, remembered items). `ExploreItems` is game state and saved.
- `ammo.rs` — a nocked arrow is the launcher's own `Inventory`, a quiver's arrows are the quiver's: never add a new holder list for them. `Fire` takes the nocked arrow first, then an *equipped* quiver.
- `tiles.rs` — sprites are names in content and saves, `SpriteId`s at runtime (never saved); `Cell.sprite` is set only on tiles in view (ADR 0023). Frames turn on the frontend's clock (`Sprite::frame`).
- `light.rs` — light is data (`render_layers`: unlit cells plus a `Lit` per tile); `render_view` bakes it in, for a frontend that can only colour cells. Only tiles in view are lit, fading with distance; no RNG, no clock.
- `fx.rs` — effects are animation-only: `GameState.fx` is never read by the simulation or saved. Push positioned ones through `fx::at`, which drops what the player can't see (ADR 0022).
- `spell.rs` — a cast runs `ModifyCast` (caster), `CheckCast` (spell, then caster), `CastRisk`, `PayCost` (whoever claimed); resources, their rules and the stat a spell scales with belong to discipline parts (`CooldownCasting`, `ManaCasting`, `BloodCasting`, a `Scroll`), never to `cast` or the spell data; pools are `value` + `as_of` computed on read. Payloads (`SpellDamage`, `Area`, `Chain`, `Summon`, `Delay`) are read in `footprint`/`cast` only, and `footprint` is shared with `aim` and `ai::spell_at` (ADR 0014). Skill (ADR 0017) is applied in `inspect_cast` only: `skill::cast` names the skills, their level strengthens the strike and steadies the fizzle chance.
- `area.rs` — pure geometry: what a shape covers is decided here and nowhere else; `spell::footprint` is the one caller, for both the cast and `aim`, so the preview and the cast agree.
- `ranged.rs`/`aim.rs` — flight is `ranged::flight` (a line, first creature or wall wins); `aim` previews through the same functions the command uses, so keep them shared (ADR 0013). Missile damage is untyped by `AfterHit`: only melee fires it.
- `ai.rs` — perception of the player goes through `can_see_player` (ADR 0007), of anyone else through `perceives` (both in the player's view, ADR 0015); never compute a view per actor.
- `side.rs` — who is an enemy is `side::enemies`, never a check for `Hostile`: summons take their owner's side (ADR 0015).
- `skill.rs` — skills are names in data and saves, `SkillId`s at runtime (ADR 0017). Store XP, derive levels (`skill::level`). Which skills an action uses is decided in one function per action kind (`skill::melee`); XP is only granted through `skill::train`, from the applier of an action that counted (enemy, stakes).
- `discovery.rs` — discoveries are checked only from `skill::train` (stakes), never from practice; a proficiency is a blueprint with `Proficiency`, attached to the learner (ADR 0018). `Discovered` is saved by name.
- `style.rs` — a known style is carried, the followed one attached (ADR 0019); styles never count toward a use's level, only train through `skill::train`. Which styles a use follows is `following` (its domains, `follows`, and `wielding` read from the hand). Principles are entries in `Style`, handled by `Style` handlers or asked with `principle`.
- `technique.rs` — techniques (ADR 0024) are attached like spells but paid in turns, `Recharging` and `Stamina`, never through disciplines. `inspect`/`ready` and `footprint` are the one check, shared by `Perform`, refused input, the list, aim and AI. A technique's blow goes through `combat::blow` like a bump; a new requirement is a `Need` variant checked in `unmet`.
- `training.rs` — lessons and practice grant XP through `skill::gain` only, never `skill::train`: no stakes, so no familiarity, styles or inspiration (ADR 0017).
- `lore.rs` — Beast lore grows only by first sightings (`sight`, after `fov::update`); `inspect` is the one place that decides what looking reveals.
- `balance.rs` — the bench's simulation (docs/plans/balance.md): a build starts as its origin (`spawn_player`, never a hand-made player); drives the player through `time::submit` like the keyboard, against dolls that never act; measures by reading the world and `fx`, never by counters added to the game. Nothing in play calls it.
- `testkit.rs` — test-only methods on `Spawn` (`thing("knife").item().at(3, 1).make(&mut state)`) and `last_log`. Add a shortcut when a component is used by more than one test; `.with(c)` covers the rest.

Everything a frontend needs is re-exported from `lib.rs`; keep that list current.


## Patterns

**One concern per part:** a component's data describes what it is or does, nothing else. How it reads (words, verbs, phrasing), how it is shown and other auxiliary data go in their own part (`Name`, `Verb`), never as extra fields on a mechanical component like `Technique` or `Spell`. The same holds for any data struct: give a separate concern its own entry instead of growing the struct.

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
