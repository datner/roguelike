# Plan: crafting

Summary: Materials with properties come out of the dives, and recipes (rules over properties, plus a few authored ones) turn them into things you could not otherwise have, at camp's workbench or, for the essentials, in the field.
Read first: `docs/vision.md` (Crafting and life skills, Camp, Interconnection), ADR 0004 (content is data), ADR 0005 (RNG streams), ADR 0008 (saves), ADR 0017 (skills, lessons), ADR 0018 (discovery), `docs/plans/legacy.md` (`Made`, the stash) and `game/CLAUDE.md`.

## Goal

The vision's crafting, its first stage: **options, never upkeep**. Today everything a character holds is
found or started with. After this plan:

- Creatures and floors yield **materials** (goblin bone, rat hide, Ember slag), items whose
  **properties** (hard, light, flexible, cold, …) are data.
- **Recipes** are data. Most are rules over properties ("a blade: an edge from anything hard, a grip
  from anything flexible"), and the product takes the properties of what went in: a slag blade burns, a
  bone one is light. A few are authored (one product from named inputs).
- Crafting is done at camp's **workbench**, except recipes marked `field` (fletching an arrow, a field
  dressing), which work anywhere.
- Three life skills grow by it: **Scavenging** (what a kill yields), **Fletching** (arrows) and
  **Smithing** (blades, armor). A new material teaches, the hundredth arrow from the same wood does not,
  and low skill makes a worse product.
- Recipes are learned by **taking a thing apart** (a lesson), from an origin, and by discovery.
- Every build may ignore all of it.

## Decisions

- **Properties are data** (`data/properties.ron`): a name and what it does to each kind of product,
  as small edits to the product's own parts: `cold` adds `Cold` to a `Weapon`'s types, `hard` raises its
  damage, `light` lowers a weapon's energy cost, `brittle` gives it a chance to break. No new combat
  rules: a crafted blade is an ordinary `Weapon`, so combat, aim and the bench read it unchanged.
- **A material is an item with a `Material { properties }` part** (ADR 0004): `GoblinBone.ron` is a
  blueprint like any other. Materials stack by kind in the inventory view, never in the world (no weight,
  no stack component: the vision's "no juggling").
- **Yields**: a creature's `Yields([(blueprint, chance_bp)])` part drops materials on its tile when it
  dies. Scavenging raises each chance. A dungeon's `items` scatter lists materials like any item, with
  `phases` for a phase's own (Ember slag only in the Embers), so phases feed crafting through data only.
- **Recipes** (`data/recipes.ron`): `slots` (a role and the properties a filling material needs),
  `makes` (the product's base blueprint), `skills` (trained, and setting the odds), `field: bool`, and
  for authored recipes `inputs` by blueprint name. Loaded and checked with the blueprints, so a typo
  fails at load.
- **Known recipes** are a `Recipes(Vec<String>)` part on the player, by name (saves and legacy keep
  names, never ids).
- **One check** (`craft::ready`), shared by the command, refused input, the recipe list and the bench,
  as `aim`/`technique::inspect` are: what the recipe needs, which carried materials fill each slot,
  where (workbench or field), and the odds.
- **`Command::Craft { actor, recipe, inputs }`** consumes the inputs and spawns the product. `inputs`
  is one choice per slot, each optional: a chosen material fills its slot, an empty one is filled by
  `ready` with the first carried material, by `SpawnSeq`, that fits. So crafting is automatic by
  default and manual when it matters (the cold shard for the edge, the common bone for the grip), and
  either way deterministic. `ready` refuses a chosen material that does not fit its slot, with the
  reason. Across the seam, a choice is an `ItemRef`. A batch reuses the slots chosen by hand and fills
  the rest anew each craft, stopping when a chosen kind runs out. One craft costs `cost::CRAFT`; a batch is a run like
  practice (`front::Session` auto), one craft a turn until the materials run out or something
  interrupts.
- **Quality**: the craft's level (weighted average of its skills, as for any use) against the recipe's
  `difficulty` decides crude, plain or fine, a `Quality` part that shifts the product's numbers (−1, 0,
  +1 on damage or armor). Drawn from a new `craft` RNG stream (ADR 0005), seeded from the world seed.
- **Growth** (ADR 0017): a craft trains its recipe's skills through `skill::train`, the subject being
  the material used (familiarity by material name), so a new material teaches and repetition decays.
  The use passes the consumption test; it has no stakes, so it never inspires except for a material
  marked `rare` (the vision: "working a rare material can inspire, common ones never do").
- **A new skill domain, `Craft`**, so crafting skills never count toward a martial or magic use, and a
  future crafting style has a domain to follow.
- **The maker's mark**: a crafted thing carries `MadeBy(name)`, the character's name. Legacy keeps it
  with `Made` (a predecessor's blade in the ruins says whose it was); relics are the judgement plan's.
- **Taking apart** is a lesson (`training.rs`): once per kind of thing, a `Disassemble` action on a
  carried item with a `Salvage { recipe, returns }` part (its own: `Teaches` names techniques) returns
  some of its materials and teaches its recipe. It
  trains the recipe's skills as a lesson, never as a use.
- **The workbench is camp data**: a tile in `data/camp.ron` (a new `Tile::Workbench`, open, drawn like
  furniture). "At the workbench" means standing next to it. The specialist places (forge, alchemist's
  table) are settlers' and later.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first.

### 1. Materials and yields — done
- `data/properties.ron` (`hard`, `flexible`, `light`, `brittle`, `cold`, `hot`, `rare` to start),
  loaded and checked with the blueprints; `Material` and `Yields` parts on `with_components!`.
- Materials: `GoblinBone` (hard, light, brittle), `RatHide` (flexible, light), `Sapling` (flexible),
  `Flint` (hard), `EmberSlag` (hard, hot, rare; Embers only), `RimeShard` (hard, cold, rare; Rime only).
  Goblins and rats yield; the caves scatter flint and saplings; the phases their own.
- `die` drops yields on the `craft` stream. Looking at a material names its properties (`lore.rs`
  `inspect`).
- Tests: a death drops what `Yields` lists at its chance (`sure` chances in tests); the phase's
  material only in its phase; save round-trip of a dropped material.
- Done. Materials are left out of the random-find pool (`Blueprints::items`), so a phase's material
  comes only from its named scatter. The save property found a real bug on the way: loading
  recomputed the view, so a game saved after the map changed (grass burned) remembered more once
  loaded. The view is now saved (`SAVE_VERSION` 65).

### 2. Recipes and the craft command — open
- `data/recipes.ron`, loaded and checked; `Recipes` part; `craft::ready` and `Command::Craft`
  (`add-command` skill), the product built from `makes` plus each property's edits, `MadeBy` on it.
- First recipes: `Arrow` (field: a shaft from anything flexible, a head from anything hard), `Blade` (a
  knife: edge hard, grip flexible), `Jerkin` (armor: two of anything flexible).
- The wanderer starts knowing `Arrow`; nobody else knows a recipe yet.
- Tests: a recipe with every slot filled makes its product and consumes exactly its inputs; a cold edge
  makes a cold blade; a chosen material fills its slot and the rest fill themselves; a chosen material
  that does not fit, a missing material, the wrong place or an unknown recipe is refused with its
  reason and costs no time; `ready` and the command agree (a property in `properties.rs`).

### 3. The workbench — open
- `Tile::Workbench` in camp; `ready` refuses a non-field recipe away from it ("You need a workbench.").
- Camp's stash keeps materials like anything left there (`Stashed`); nothing new.
- Shots: camp with the workbench.

### 4. Growing by it — open
- `Craft` domain; `Scavenging`, `Fletching`, `Smithing` in `data/skills.ron`; recipes name theirs.
- A craft trains through `skill::train` with the material as subject; Scavenging trains on each yield
  and raises yield chances; `Quality` from the craft level against `difficulty` on the `craft` stream.
- Rare materials can inspire (`discovery::inspire` after a craft that used one).
- Tests: a new material trains more than a repeated one; low level makes crude more often than high
  (over seeds); a rare material's craft can inspire and a common one never does.

### 5. Crafting from the keyboard — open
- `add-ui` skill: a recipe list mode (what you can make now first, then the rest with what each lacks,
  from `craft::ready`), Enter crafts one with the materials filled in, a count crafts a batch as a run.
  Tab opens the recipe's slots, each showing the material `ready` picked: choose another for a slot
  (only what fits is listed), then Enter. A key not yet bound.
- The inventory groups materials by kind with a count; looking shows properties.
- Tests by driving a `Session`; shots for the list.

### 6. Taking apart and new recipes — open
- `Disassemble` (a lesson): once per kind, returns some materials and teaches the recipe its `Salvage`
  names; the goblin archer's crude bow teaches `ShortBow`, a rusty knife `Blade`.
- `discovery` grants recipes (`Grant::Recipe`); a first one: Fletching 5 + Bow 5 inspires
  `BroadheadArrow`.
- Tests: once per kind; the grant lands; a save keeps known recipes.

### 7. The bench and the vision — open
- The floor bot picks up materials and the Floors report counts them per floor (materials a dive banks);
  a `fletcher` build crafts arrows at camp between dives in the dive runs.
- The vision's Crafting section gets "Built so far" and what moved to later plans.

## Out of scope

- Cooking and its dive-bounded timing (a new kind of timing to name: its own plan).
- Scribing, alchemy, tinkering and their places (settlers' places, after the settlers plan).
- Wear and repair (the vision allows wear only where a phase makes it a choice).
- Crafting styles, renowned makers' relics, perception tags on crafts (judgement, factions).
- Trade.

## Connections

**Reads**
- phases: a dungeon's `items` scatter limits a material to its phase (`Scatter.phases`), so each phase's material exists only in its dives *(planned: crafting slice 1)*
- dives: materials come only from floors (`Yields` on death, the dungeon's scatter) and are banked by bringing them to camp's stash *(planned: crafting slice 1)*
- skills: the craft level from the recipe's skills sets `Quality` in `craft::ready`; Scavenging raises yield chances *(planned: crafting slice 4)*
- training: taking apart is a lesson in `training::study`, teaching a recipe once per kind *(planned: crafting slice 6)*
- discovery: `Grant::Recipe` teaches a recipe at a moment of inspiration *(planned: crafting slice 6)*

**Read by**
- combat: a crafted thing is an ordinary `Weapon`/`Armor`/`Ammo` whose numbers and damage types come from its materials' properties, read by `combat::strike` and armor resistance like any other *(planned: crafting slice 2)*
- skills: crafting trains its recipe's skills through `skill::train`, the material as subject *(planned: crafting slice 4)*
- discovery: a craft with a rare material calls `discovery::inspire` *(planned: crafting slice 4)*
- origins: an origin's blueprint lists the recipes it starts knowing in `Recipes` *(planned: crafting slice 2)*
- legacy: `MadeBy` stays on a crafted thing in the ruins, so a predecessor's work says whose it was *(planned: crafting slice 2)*
