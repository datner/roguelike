# Plan: origins

Handoff document. Read `docs/vision.md` ("The shape of the game", "The character", abilities),
`docs/plans/dives.md` (where origins were deferred), ADR 0004 (content is data), ADR 0017 (skills),
ADR 0019 (styles), ADR 0024 (techniques) and `game/CLAUDE.md` first.

## Goal

No classes: a character starts from an **origin**, what they are. An origin gives a few starting skills,
a kit, perhaps a style already in progress, and **abilities**: things that come from what the creature
is, never learned and never found on a scroll. Today every character is the same fixed `Player`
blueprint; after this plan a new game asks which origin, and the origin decides the start. Origins are
also what legacy will build on (a new character is never stronger for its predecessor; its origin is
its whole head start).

## Decisions taken for this plan

- **An origin is data**, listed in `data/origins.ron` in the order the menu shows them: a key, the name
  the player reads, a one-line summary, and the blueprint the character is made from. Each origin's
  blueprint `inherits: "Player"`, so the body, stats, disciplines and `Learner` stay in one place and an
  origin only adds what makes it different (skills, `equipment`, `techniques`, `spells`, a known
  style). Loaded and checked with the blueprints; an origin naming a missing blueprint, or one that
  does not inherit `Player`, fails at load. The first origin is the default: `GameState::new`, the tests
  and the bench start from it.
- **The `Player` blueprint becomes the bare person**: body, stats, the disciplines, `Learner`, no kit,
  no spells, no techniques. Everything a character holds or knows at the start comes from its origin.
- **The origin is remembered**: an `Origin(key)` component on the player, saved by key, shown on the
  character sheet. Legacy will read it.
- **A starting style is known, maybe followed.** An origin's `styles:` list spawns them carried (known);
  `follow:` names one to take up from the start. "A style in progress" is its skill at a level, set
  like any other skill.
- **Abilities are marked, not a new mechanism.** An `Ability` marker on a blueprint (a technique, or a
  passive part like a proficiency) says it comes from what the creature is. Load refuses an ability
  named by a manual (`Teaches`), a discovery or a style's `invents`; the techniques list and the Styles
  panel show abilities apart from what was learned. Monsters' natural techniques (the spirit wolf's
  trip is the general trip, so it stays as it is) may be marked later; nothing depends on it.
- **Choosing is a menu step**, keyboard first: New game → name → origin (a list with each origin's
  summary and what it starts with, from a `game` view so every frontend says the same) → play. Loading
  a save never asks. `Action::Play` gains the origin for a new character.
- **Spells move to an origin.** Today every character knows seven spells, a leftover from building
  them. They go to the hedge mage; the wanderer keeps today's bow, quiver, torch and trip. This changes
  what a default new game can do, and the `spells`/`aim` shots are rebuilt from the hedge mage.
- **Determinism:** the origin is part of the new game's input, like the seed. Its kit spawns without
  RNG, so a seed's world is the same for every origin; only `SpawnSeq` numbers after the player shift.
- New ADR **0025 — Origins**, written in slice 1 and amended per slice.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first. Every new
component: blueprint `ComponentDef`, `SavedComponent` + round-trip test, `SAVE_VERSION` bump.

### 1. An origin, end to end in `game`: the wanderer — done
- `game/src/origin.rs` (new): `OriginDef { key, name, summary, blueprint, styles, follow }`, read from
  `data/origins.ron` and checked at load (blueprint exists and inherits `Player`; styles are style
  blueprints; `follow` is one of them). `Blueprints::origins()`.
- `GameState::new_as(seed, blueprints, origin)`; `GameState::new` uses the first origin. `with_map`
  spawns the origin's blueprint where it spawns `Player` today, then its styles, then stocks quivers.
- `Origin(key)` on the player, saved by key.
- Content: `Player.ron` loses its kit, spells and techniques; `Wanderer.ron` (`inherits: "Player"`)
  takes the bow, quiver, torch, the trip and, for this slice only, the spells, so nothing in play
  changes yet.
- ADR 0025.
- Tests: the first origin is the default; an origin's kit, skills and style are on the new player;
  the same seed builds the same world for two origins; a save keeps the origin; a bad origin file
  fails at load with its message.

### 2. Abilities — done
- `Ability` marker component (blueprint `ComponentDef`, saved). Load refuses an ability in `Teaches`,
  a discovery's grant or a style's `invents`.
- `view::techniques` rows carry `ability: bool`; the Styles panel (`front/overlay.rs`) lists abilities
  under their own heading; the technique chooser marks them.
- Tests: the load refusals, each with its message; an ability is listed apart; it performs like any
  technique.

### 3. Choosing an origin — done
- `game::origins(blueprints) -> Vec<OriginView { key, name, summary, starts_with: Vec<String> }>`: the
  kit, skills, techniques, spells, styles and abilities in plain words.
- `front/menu.rs`: after a new name, an origin screen (↑↓, Enter, Esc back to the name); `Action::Play`
  carries `Option<origin>` (None for a save). `title::start` passes it to `GameState::new_as`.
- `gui`: `menu_view.rs` draws the list and the focused origin's summary and start; a `origins` shot.
- The character sheet shows the origin.
- README Controls.
- Tests: a `Menu` flow name → origin → play, and Esc back; a loaded save never asks; the shot.

### 4. The origins
- **Wanderer** (first, default): short bow, quiver, torch; Archery 1; the trip. Spells leave it.
- **Duelist**: rapier, nothing in the off hand; Pierce 2, Sword 1, One-handed 2, Empty offhand 2;
  Fencing known and followed at level 1. Ability: none, the style is its gift.
- **Hedge mage**: torch; Magic 2, Pyromancy 1, Arcane 1; firebolt, magic missile and the arcane lance
  (the other spells stay unlearned until magic styles invent them, ADR 0016).
- **Pit fighter**: leather armor, torch; Melee 2, Unarmed 2, Dodge 1; Brawling known. Ability:
  **headbutt** (`Headbutt.ron`: an unarmed blow, `damage_pct: 25`, the off-balance rider, recharge 8
  turns, `Ability`).
- Tests that relied on the player's spells or kit build the origin they need (`GameState::new_as`); the
  `spells` and `aim` shots start as a hedge mage.
- Tests: each origin loads and starts as described; the headbutt is an ability and performs.

### 5. The bench starts from origins
- `Build.origin: Option<String>` (default: the first origin) replaces the hard-coded `Player`; the
  build's `equipment`, `techniques` and `style` still replace or add as today.
- One starter build per origin, so the report compares where each origin begins.
- `balance::cost` names "origin: <name>" as a source when an origin starts with the thing.
- Tests: a build starts from its origin; the starter builds load; the cost source.

## Out of scope

- Origins with other bodies (a winged or four-armed origin): `Body` comes from the blueprint, so it is
  data when wanted, but no content now.
- Origin-specific stats beyond what a blueprint sets; point-buy or random rolls at creation.
- Legacy (the record, ruins, capped lessons): its own plan, reading `Origin`.
- Marking monsters' natural techniques as abilities.
- Balancing origins against each other beyond the bench's first report.
