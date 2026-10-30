# Plan: spells and magic systems

Handoff document. Read `CLAUDE.md`, `game/CLAUDE.md`, `docs/decisions/0014-spells-and-disciplines.md`,
and `docs/decisions/0006-time-model.md` first. This plan is self-contained beyond those.

## Goal

Add spells to the roguelike in slices, from simple to complex. Different schools or
classes must be able to pay for and cast spells in different ways (cooldowns, mana, heat,
reagents, HP). Spells must support single-target bolts, areas, cones, lines and summons.

## Ground rules (from the project; do not break)

- `game` never depends on a terminal crate. `term` talks to it through `PlayerAction` and plain views.
- All world mutation goes through `Command` / `apply_commands` (ADR 0002). Event handlers get
  `&World` and emit commands; they never mutate.
- Determinism: all randomness from `GameState.rngs`, one stream per subsystem (ADR 0005); no
  `HashMap`/`HashSet`, no clock. A new subsystem gets a new stream id in `rng.rs`.
- Content is RON blueprints composed from reusable parts (ADR 0004).
- Timed state is `start` + `duration` read against `GameState.now`; never countdowns (ADR 0006).
  Ties break on `SpawnSeq`, never `Entity` order.
- No `unwrap`/`expect` in library code. Tests sit next to the code.
- Every slice ends with `scripts/check.sh` green. Never run `term` (it blocks); test UI with
  ratatui `TestBackend`. Do not commit unless asked. Commits are conventional and scoped by crate.
- Use `scripts/map.sh --sigs <path>` to see APIs; read files with offset/limit.
- Skills in `.claude/skills/`: `add-command` and `add-part` are recipes for the usual steps
  (variant, handler, cost, input, save, blueprint, tests).

## Design (decided)

**Separate what a spell does from how it is paid for.**

1. **Spell** = a blueprint entity attached to its caster (`Attachments`, not `Inventory`).
   It holds targeting (`Spell { school, range, power }`) and payload parts (`SpellDamage`, later
   others). `power` is an abstract magnitude. A spell never names a resource.
2. **Discipline** = a part on the caster with handlers for two events:
   `CheckCast` (claim the school, or veto with a reason) and `PayCost` (emit commands that take
   the price). A school no discipline claims cannot be cast. Handler priorities live in `prio.rs`.
3. **Per-spell state** (e.g. `Recharging(Timed)`) lives on the spell entity; **pools** (mana, heat)
   live on the caster. Lazy regeneration: store `value` + `as_of` tick and compute on read; no
   per-tick timer.
4. **One `Command::Cast { actor, spell, at }`**, costing `cost::CAST`. `spell::ready_cast` runs the
   check on a cloned RNG stream and a scratch buffer so the input check and any UI preview cannot
   disagree with the cast.
5. **Targeting layers** (for slice 3 onward): target mode (self / point / direction) -> `area()`
   (a pure function returning an ordered tile list, using `fov.rs` shadowcasting for bursts and
   cones, `ranged::line` for beams) -> payload applied per affected creature, ordered by `SpawnSeq`,
   emitting commands (collect-then-apply).
6. A planned `ModifyCast` event between check and pay lets equipment and perks adjust range, cost
   or power. Add it only when something needs it.

## Current state: every slice is done, as far as this plan goes

Slices 1, 1b and 2 are committed (`feat(game): spells and the cooldown discipline`, `feat: cast from
the terminal, mana and magic stats`). Slices 3 to 6 are in the working tree, `scripts/check.sh` green.
What is left is proposed in ADR 0016 (terrain overlay, element interactions, runes) or listed under
"Left for later" below.

Slice 1b as built: `CheckCast` carries `power` and `costs: Vec<CostLine>` (a claiming discipline
describes its price there); `view::spells` gives
`SpellView { spell, name, range, damage, preview: CastPreview { costs, blocked } }`; `aim` has
`Shot::Cast` with its own small `Ready`; `term` has `c`, `Modal::ChooseSpell`, and a chooser that takes
`&[ItemRef]` plus pre-built lines.

Slice 2 as built (owner: each discipline scales with its own stat, now; the second school is the
player's): `Stat`/`Stats` gain Intelligence and Willpower (`Stats::AVERAGE` for tests). The discipline,
not the spell, names the stat (`CooldownCasting.stat`, `ManaCasting.stat`, set on `CheckCast.stat`);
`SpellDamage.stat` is gone. `Mana { value, max, as_of, regen_ticks }` + `Command::SpendMana`, priority
`prio::MANA`. `CheckCast`/`PayCost` name the `caster`. `spell::inspect_cast` returns the cast plus an
optional refusal; `ready_cast` wraps it. Handlers push their `CostLine` before vetoing. Blueprint load
rejects two disciplines for one school (`SchoolClaimedTwice`). Content: `School::Arcane`,
`MagicMissile.ron` (power 1, 2-4, range 8); the player has INT 18 / WIL 18, firebolt under Willpower,
magic missile under 3 mana per power from a 10-point pool regaining 1 per 200 ticks. The side panel
shows MP and INT/WIL. `SAVE_VERSION` 12.

Slice 3 as built: `Area { shape, hits, falloff }` spell part (no `Area` = bolt); `Shape` = `Burst { radius }`
(flies, bursts where it lands), `Cone` (fixed 90°), `Beam` (through creatures), `Nova { radius }` (on the
caster, not aimed). Target mode follows from the shape (`spell::check_target`), so `PlayerAction::Cast`
needed no change. `area::tiles` is pure (walls block via `fov::compute`; radius 1 = 3×3, larger round),
ordered by distance from the centre then row-major (also the animation order). `spell::footprint` gives
flight, tiles, centre and the caught creatures (`SpawnSeq` order, filtered by `Hits`: `All` / `Enemies` /
`Allies`, side = `Hostile` or not) to both `cast` and `aim`; `Aim` gained `area` and `caught`. One combat
roll per caught creature. Content: `Fireball.ron` (pyromancy burst 1, hits all), `ArcaneLance.ron`
(arcane beam, enemies). `SAVE_VERSION` 13.

Slice 1 done:
- `game/src/entity.rs`: `School` (only `Pyromancy`), `Spell`, `SpellDamage { min, max, stat: Option<Stat> }`,
  `Recharging(Timed)`, `CooldownCasting { school, ticks_per_power }`.
- `game/src/spell.rs`: `CheckCast`, `PayCost`, `register` (cooldown handlers, `prio::COOLDOWN`),
  `ready_cast`, `check_cast`, `cast`, `recharge`, tests.
- `command.rs`: `Command::Cast`, `Command::Recharge`, `DamageCause::Spell` (untyped, so armor passes
  it), input check in `reject_input`, `dispatch` made `pub(crate)`.
- `cost.rs` `CAST`; `ranged.rs` `check_range` and `position` shared with spells.
- `PlayerAction::Cast { spell: ItemRef, at }` in `input.rs`, handled in `time.rs`;
  `view::known_spells`.
- `data/entities/Firebolt.ron`; `Player.ron` has `CooldownCasting(school: Pyromancy, ticks_per_power: 300)`;
  `GameState::new` attaches Firebolt to the player (`teach_spells`); the shared `test_blueprints()`
  has a Firebolt entry.
- Blueprint (`ComponentDef`) and save (`ComponentKind`/`SavedComponent`) registration; `SAVE_VERSION` 11.
- ADR 0014, decisions index, `game/CLAUDE.md` rule.

Not done: any terminal UI. The player cannot cast in the real game yet.

## Slices

Each slice: tests first where practical, then code, then docs (ADR update or new ADR if a decision
changes, `README` Controls if user-visible, `scripts/map.sh`), then `scripts/check.sh`.

### 1b. Cast from the terminal
- `game::view`: `SpellView { spell: ItemRef, name, school, range, ready: Result<(), String> }` and
  `spells(state)` listing known spells (plain data, no hecs types). Include a generic
  `CastPreview { costs: Vec<CostLine>, blocked: Option<String> }` shape now, even though only
  the cooldown line exists, so later disciplines need no UI change.
- `game::aim`: add `Shot::Cast(ItemRef)`; `ready`, `aim_range`, `aim_targets` and `aim` handle it using
  `spell::ready_cast` and `ranged::flight`. Damage preview is the dice range (spells ignore armor).
- `term`: a `c` key opens a spell list (reuse `targeting.rs`'s chooser), then the aim cursor, then
  `PlayerAction::Cast`. Draw the bolt with the existing `Trail` animation. Show cooldown state in the list.
- Tests: `TestBackend` rendering of the list and aim; `game` tests that `aim` for `Shot::Cast` agrees with `cast`.
- README Controls: add `c`.

### 2. Mana, the second discipline (the real test of the seam)
- New parts: `Mana { value, max, as_of, regen }` (or similar) on the caster and `ManaCasting { school, mana_per_power }`.
  Register `CheckCast` (veto "Not enough mana.") and `PayCost` (emit a `Command` that spends mana and
  updates `as_of`). Pick a priority in `prio.rs`.
- A school is claimed by exactly one discipline; decide and document what happens if two claim it
  (recommended: the first by priority claims, a second claiming the same school is a content error
  caught by a blueprint-load or test check).
- Add a second school (e.g. a `Hedge` or `Arcane` school) with a spell under mana, so the player or an
  NPC demonstrates two disciplines side by side.
- **Acceptance:** adding mana requires no change to `spell::cast` or `Spell` data. If it does, fix the seam first.
- Add a magic stat (e.g. `Willpower`/`Intelligence`) only if you want scaling now; `SpellDamage.stat` already allows it.
  Note `SAVE_VERSION` bumps and `Stat`/`Stats` ripple (`view::character_sheet`, blueprint, save).
- Status line / side panel shows mana. ADR 0014 gets a paragraph.

### 3. Areas and shapes
- New module `area.rs`: `area(shape, caster, aim, &Map) -> Vec<(i32, i32)>`, pure and ordered row-major.
  Shapes: single, burst (radius around a point, line-of-sight from the centre), cone (angle + range, shadowcast
  from the caster), beam (line that passes through creatures). Walls block.
- Target modes: `Point { range }`, `Direction`, `Self`. Extend `PlayerAction::Cast` / aim for direction aiming.
- Payload applies per creature in `SpawnSeq` order, emitting `DealDamage` commands. Add a `hits:` policy
  (enemies / all / allies) and optional distance falloff.
- `aim::Aim` returns the affected tiles so the UI highlights exactly what will be hit.
- Write the pure-function tests first: wall blocking, cone edge tiles, ordering, friendly-fire policy.

### 4. Allegiance, then summons
- Minimal `Faction` (or owner-based allies). `ai.rs` target selection and the `hits:` policy read it. Today the
  AI is player-relative; this is a prerequisite and a slice of its own with tests (AI ignores allies).
- `Summon { blueprint, count, placement, lifetime }` payload: spawn through `GameState::spawn` (the only way),
  placement = nearest free tiles in a fixed order, ties from a spell RNG stream (new id in `rng.rs`).
- Lifetime as a `Timed` effect on the summon; expiry via `effect::expire_effect`; `ModifyDuration` extends it.
- `Summoned { owner }` component, saved; despawn with the owner. A summon cap is a `CheckCast` veto from a discipline.

### 5. More disciplines and costs (pick by what the game needs)
- Fizzle risk: a roll from a new `spell` RNG stream, affected by stats and armor weight.
- Materials: scrolls (single-use items), wands (charges that recharge by timestamp), reagents consumed from
  `Inventory`, focus items in equipment slots that hook `ModifyCast`.
- Heat/overload: casting adds Heat that decays by timestamp; over a threshold gives backfire or a debuff.
  Variant: spells cost HP.

### 6. Complex spells
- Chain (repeatedly pick the nearest unhit creature in sight, fixed tie-break), delayed blast / trap
  (a Timed pending entity that resolves at `start + duration`), terrain-making walls (needs a temporary
  tile-overlay layer on the map), projectile spells reusing `ranged` flight.
- Elemental damage types: extend `DamageType`/`Armor` (ADR 0012) with resists; `DamageCause::Spell` becomes typed.
- Element interactions (wet + shock, oil + fire) need tile/entity state; plan separately, with an ADR.
- Rune composition: Form x Element x Modifier, cost computed from parts. Treat as one more discipline plus a
  spell-assembly step.
- AI casting: an AI part that asks `ready_cast` and chooses a spell (e.g. a goblin shaman).

## Open questions for the owner

- Preferred second non-mana resource after slice 2 (heat, reagents, HP).
- Whether mana regeneration should scale with a stat too.

## Definition of done per slice

`scripts/check.sh` passes; new parts are registered in blueprint and save with a `SAVE_VERSION` bump and a
round-trip test; ADR 0014 (or a new ADR) is updated when a decision changes; `docs/MAP.md` regenerated; a short
report listing what changed, what was left out, and follow-ups.

## Slices 4 to 6 as built

- **4a, sides** (ADR 0015): `side::side` (hostile or the player's; a summon takes its owner's),
  `side::enemies`. AI goes for the nearest perceived enemy; perception of non-players is "both in the
  player's view, in radius, open line". `Hits`, `aim_targets` and explore read sides. The player swaps
  places with their own summon.
- **4b, summons** (ADR 0015): `Summon { blueprint, count, lifetime }` (name in RON, id at runtime,
  name in saves), placement on the free tiles nearest the spot with ties shuffled from the new `spell`
  RNG stream, `Summoned { owner }`, a `Binding` effect (`Binding.ron`) whose expiry dismisses
  (`Command::Dismiss`), dismissal on the owner's death, `SummonLimit` veto. `SummonWolf.ron`,
  `SpiritWolf.ron`.
- **5, paying** (ADR 0014): `ModifyCast` + `Focus` (`EmberCirclet.ron`), `CheckCast` asked of the
  spell first (`Scroll` claims and pays by `Command::UseUp`; `ScrollOfFireball.ron`), `CastRisk` +
  `Fizzle` (armor weight against Willpower; the player has it), `BloodCasting`.
- **6, complex** (ADR 0014): elemental `DamageType`s with `Armor` resistances and typed
  `DamageCause::Spell`; `Chain` (`ChainLightning.ron`); `Delay` → `Rune.ron` + `Blast`, bursting
  through `effect::expire_effect` (`FireRune.ron`); `spells:` in blueprints; `SpellAi` +
  `ai::spell_at`; `GoblinShaman.ron` with `BloodBolt.ron`, one per zone after the items.
- `SAVE_VERSION` 17; 18 after mana growth (`Mana.growth`: Intelligence deepens the pool, Willpower
  quickens it).

## Left for later

- Terrain-making spells, element interactions, rune composition: ADR 0016 (proposed).
- Heat/overload, wands with charges, reagents from `Inventory`.
- Cone widths other than 90°.
- AI casting area spells is allowed but untested beyond "spares its own side"; shamans only know bolts.

## Connections

**Reads**
- skills: `skill::cast` picks the skills a cast uses; `skill::steadied` lowers its fizzle.
- combat: a spell's damage is a `Command::Damage`, finished by armor and resistances like a blow's.

**Read by**
- lore: `lore::first_cast` trains Arcana; `lore::inspect` lists the spells a creature knows.
- ai: `spell::ready_cast` and `spell::check_target` decide a caster's turn (`ai.rs`).
- techniques: a technique's shape and recharge are the spell's (`spell::cover`, `spell::recharge`).
- phases: the element lean multiplies a spell's typed damage *(planned: phases.md, slice 1)*.
- crafting: reagents and scribing scrolls *(vision)*
