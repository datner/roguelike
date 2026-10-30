# Plan: phases

Handoff document. Read `docs/vision.md` ("Interconnection", "Phases", "The character"),
`docs/plans/dives.md` (where phases began), ADR 0004 (content is data), ADR 0012 (armor and typed
damage), ADR 0014 (spells), ADR 0016 (elements, proposed), ADR 0017 (skills), ADR 0018 (discovery),
ADR 0019 (styles) and `game/CLAUDE.md` first.

## Goal

Today a phase is a name and a line of text for waiting, and it changes only which spawn entries
appear. After this plan each phase leans the whole world: its element is stronger for everyone, it
bends one rule, it has its own natives and finds, and the character systems read it. Fighting in a
phase attunes you to it, a style can thrive in one, and a discovery can need stakes in one. There are
five phases: the Stillness, which looks neutral, and two opposed pairs.

## Decisions taken for this plan

- **The five phases** (`data/phases.ron`, in wheel order; the first is where a world starts):

  | Phase | Element | Rule it bends | Opposite |
  |---|---|---|---|
  | Stillness | none | none | none yet |
  | Embers | Fire | light reaches farther (the dark glows) | Rime |
  | Rime | Cold | stamina recovers more slowly | Embers |
  | Tempest | Shock | missiles lose accuracy with distance | Mire |
  | Mire | Acid (new) | moving costs more energy | Tempest |

  Acid is a new `DamageType`. It also lets the Mire reach into armor (ADR 0012); its natives corrode
  rather than burn. The Stillness has no element, rule or opposite **on purpose**. It is not modelled
  as "no phase": it is a `Phase` like the others whose lists happen to be empty, so revealing it later
  is new data on an existing entry, not a new kind of thing.
- **An element is one kind of lean, not what a phase is.** Later phases (Lush, Forgotten, Shining,
  Regretful, Hungry) lean other systems and may have no element. So `element` is optional and holds no
  special place: the lean and rules sit beside it, and nothing may treat "the phase's element" as the
  phase's identity (no `match` on element to decide behaviour, no UI that assumes every non-Stillness
  phase has one).
- **The element lean is symmetric**: in a phase with element E, all damage of type E is multiplied by
  the phase's `lean` (data; first guess +50%), whoever deals it. It is applied in the one function
  where typed damage is finished, which the shot, the cast, the technique and the previews share, so
  `aim` and `preview_damage` agree with the blow.
- **A rule is data, read where its system already decides**: `Rule` is an enum with a strength
  (`LightReach(pct)`, `StaminaRecovery(pct)`, `RangedFalloff(pct per tile)`, `MoveCost(pct)`). Each
  variant is read in exactly one place: `light.rs`, the `Stamina` pool's rate, `combat::odds` for
  missiles, `cost.rs` for moves. A phase lists any number of rules; today each has one or none.
- **The current phase has one accessor**: `phase::current(state) -> &Phase`. Every system that reads the
  phase goes through it, so how the phase is decided can change without touching the readers. A
  dive keeps its phase (unchanged); camp reads the phase being waited for.
- **Choosing a phase is behind one seam**: `phase::choices(state) -> Vec<PhaseChoice>` lists what can be
  chosen and why not, and `WaitForPhase` is the one command that sets the next phase. Today every phase
  is a choice at camp. Nothing else (menu, session, gui, bench) may assume waiting, a list, or that all
  phases are always available. A cycle, a ritual or an earned phase replaces these two functions and
  nothing else.
- **Natives and finds are dungeon data**, as today: spawn entries with `phases:`. Each new phase gets at
  least one native kind (resistant to its element, and dealing it) and one find only it has. A new
  kind is novelty, so Beast lore and familiarity reward visiting a phase you haven't seen (pillar 2),
  with no extra code.
- **Attunement is a skill per phase** (`Embers attunement`, …) in `data/skills.ron`, under a new
  `attunement` domain. A use with stakes during a phase also trains that phase's attunement, added in
  `skill::train` itself, so every kind of action counts the same. Attunement pays off in one place:
  it softens the phase's rules **for you** (each level takes a % off a rule's strength, capped). Your
  element lean is untouched: it stays symmetric. The Stillness has no attunement yet.
- **Styles and discoveries read the phase**:
  - `Style.thrives: Vec<phase>`: a use that follows the style gains more style XP in those phases (data
    multiplier). It's a data field read in `style.rs`'s training, beside `wielding`.
  - `Requirement::Phase(key)` in `data/discoveries.ron`: met only while in that phase, so the moment of
    inspiration must happen there, with stakes.
  - Content: at least one style that thrives in a phase, and at least one discovery that needs one
    (e.g. Pyromancy + Embers attunement, with stakes in the Embers, inspires a fire style or a
    proficiency).
- **Legacy reads the phase too**: a record's `place` already names it; the grave note says it.
- **The bench stays in the Stillness** by default, so this plan moves no existing number.
  `Scenario.phase` lets a scenario name one.
- New ADR **0027 — Phases**, written in slice 1 and amended in each slice.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first. Every new
component: blueprint `ComponentDef`, `SavedComponent` + round-trip test, `SAVE_VERSION` bump. Run
`scripts/balance.sh` after any slice that changes numbers.

### 1. The seam and the lean
- `Phase` gains `element: Option<DamageType>`, `lean: u32` (%), `rules: Vec<Rule>` (empty until slice
  3) and `opposite: Option<String>`, checked at load (an opposite exists and names it back).
- `phase::current`, `phase::choices`; the camp phase chooser (`front`) lists `choices`, and a refused
  choice shows its reason.
- `DamageType::Acid`; the lean applied where typed damage is finished.
- Content: the five phases in `phases.ron` (Rime, Tempest and Mire added; Embers leans Fire).
- ADR 0027.
- Tests: the lean multiplies matching damage for player and monster alike, and nothing else; preview
  equals the blow in a leaning phase; the Stillness changes nothing; a bad opposite fails at load;
  `choices` lists every phase today.

### 2. Natives and finds
- Blueprints: one native per new phase (e.g. a rime wisp, a storm-hawk, a bog crawler), plus an ember
  native beside the shaman. Each resists its element and deals it; Acid wears armor.
- One find per phase (a manual or an item that only spawns there), added to dungeon spawn entries.
- Tests: each phase's floor spawns its natives and never another phase's; the natives load with their
  resistances.

### 3. Rules
- `Rule` variants and their one reader each: `LightReach` (`light.rs`), `StaminaRecovery` (the stamina
  pool's rate), `RangedFalloff` (`combat::odds` for missiles, so `aim` shows it), `MoveCost`
  (`cost.rs`).
- Content: one rule per elemental phase, as in the table.
- Tests: each rule changes its one number in its phase and not elsewhere; the aim preview shows the
  falloff.

### 4. Attunement
- `attunement` domain and one skill per elemental phase in `skills.ron`; `skill::train` adds the current
  phase's attunement to every use with stakes.
- Attunement level softens each rule for its holder (in the same one reader, through
  `phase::rule_strength(state, who, rule)`).
- Tests: a use with stakes in the Rime trains Rime attunement, one at camp or in practice doesn't; a high
  attunement softens the rule for the player and not for a monster; it never touches the lean.

### 5. Styles and discovery
- `Style.thrives` and its XP multiplier; `Requirement::Phase` and its check in `discovery::inspire`.
  `discovery::ideas` shows a phase requirement as met or not (and where to go).
- Content: one thriving style, one discovery that needs a phase.
- Tests: style XP is higher in a thriving phase; the discovery fires only in its phase, with stakes.

### 6. Seeing it
- The camp chooser shows each phase's element, rule and opposite from a `game` view (`PhaseView`
  grows), built into text in `front/text.rs`. The status line names the phase and its element on a
  floor. The sheet lists attunements as skills.
- gui: the chooser and status; refreshed `camp`/`phases` shots.
- README Controls and Structure.
- Tests: the chooser text; a `Session` flow that waits for a phase and descends into it.

## Out of scope

- How a phase is really chosen (a cycle, a ritual, earned phases): the seam is built, the mechanism is
  not.
- Revealing the Stillness, and Chaos.
- Phases changing the dungeon's shape (lava seams, lakes): generator settings per phase come later.
- Terrain overlays and element interactions (ADR 0016: wet, oil, burning ground).
- Phase lore (inspecting a phase), legacy weathering that depends on the phase.

## Connections

*TODO* (interconnection.md, slices 2–3)
