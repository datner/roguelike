# Plan: techniques

Handoff document. Read `docs/vision.md` ("The character", especially Styles and "Techniques and
abilities"), ADR 0014 (spells), ADR 0017 (skills), ADR 0019 (styles) and `game/CLAUDE.md` first.

## Goal

Special actions for the body and the blade: a lunge, a cleave, a shove. Today a creature fights only by
bumping, and styles change how it reacts (a riposte) but never give it anything new to *do*. Techniques
are that missing link: `skills → styles ──invent──> techniques`. They are learned from a manual,
invented by a followed style, or, for a creature, simply known from its blueprint (an ability).

## Decisions taken for this plan

- **A technique is a blueprint entity attached to its holder**, like a spell (ADR 0014): in use, saved
  and despawned with them. `Technique { domain, reach, recharge }` says how it is aimed and its price;
  payload parts say what it does.
- **It shares the spell's effect half, not its source or payment.** Areas reuse `Area` and `area::tiles`;
  aiming reuses `aim` with a new `Shot::Technique`. No discipline, no `CheckCast`/`PayCost`, no scroll.
- **The price is time.** A technique costs a turn (`cost::PERFORM`) and then recharges: `Recharging(Timed)`
  on the technique entity, the type spells already use, read from timestamps (ADR 0006). No stamina pool:
  it would be a fourth resource before the second one (mana) is fully used. Revisit if recharges alone
  make techniques a rotation instead of a choice.
- **A technique's blow is a blow.** It strikes with the wielded weapon through the same `combat::odds` /
  `combat::resolve` / roll as a bump, so skills, grazes, dodges, deflection and on-hit parts all apply. A
  `Blow` part only shifts it (accuracy, damage). It trains the blow's skills through `skill::after_blow`,
  with stakes as usual; a technique has no skill of its own.
- **A technique may require the blow's skills** (`needs: ["Pierce"]`): the lunge needs a thrusting weapon.
  Checked against `skill::melee` of the current strike, so swapping weapons decides what you can do.
- **Not tied to the followed style.** An invented technique stays usable when you follow another style;
  what it needs is in its `needs`. Switching styles stays about principles.
- **Abilities need no new mechanism**: a creature's `techniques:` list in its blueprint (like `spells:`).
  What distinguishes an ability (it comes from the body or origin) waits for origins.
- **Key:** `a` (act) opens the technique list, then the aim cursor, like `c` for spells. Free in `gui`.
- New ADR **0024 — Techniques**, written in slice 1 and amended per slice.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first. Every new
component: blueprint `ComponentDef`, `SavedComponent` + round-trip test, `SAVE_VERSION` bump.

### 1. A technique, end to end in `game`: the lunge
- `entity.rs`: `Technique { domain: Domain, reach: i32, recharge: u64, needs: Vec<SkillId> }` (saved by
  name like `Style`), `Blow { accuracy: i32, damage_pct: i32 }`, `Dash { tiles: i32 }`.
- `command.rs`: extract the body of `attack` into `combat::blow(state, attacker, target, &BlowMods)`; a
  bump is `blow` with no mods, so existing combat tests pin it.
- `technique.rs` (new): `ready(state, actor, technique) -> Result<Ready, String>` (recharging, needs met,
  wielding a weapon if `needs` is not empty) and `check_target(state, actor, technique, at)` (an enemy in
  reach, on a straight or diagonal line, the tiles between open and empty when it dashes). One shared check,
  as `spell::ready_cast`: `reject_input`, `aim` and AI all call it.
- `PlayerAction::Perform { technique: ItemRef, at }` → `Command::Perform { actor, technique, at }`
  (`cost::PERFORM = STANDARD`): dash along the line (plain position change, no energy, no RNG), `blow`
  with the `Blow` mods, then `Command::Recharge`. Logs "You lunge at the goblin." through `Subject`.
- Content: `data/entities/Lunge.ron` (reach 2, dash 1, `Blow(accuracy: 2, damage_pct: 25)`, needs
  Pierce, recharge 5 turns). For now, given to the player blueprint so it can be tested in play;
  slice 6 takes it away again.
- `view::techniques(state) -> Vec<TechniqueView>` (name, ready or why not, turns until recharged).
- Tests: lunge hits from two tiles and lands adjacent; refused when recharging, out of reach, off-line,
  blocked, with no target, with a slashing weapon (each with its message); the recharge expires on time;
  a seeded lunge draws the combat stream like a bump; save round-trip mid-recharge.
- ADR 0024.

### 2. From the keyboard, with a preview
- `aim.rs`: `Shot::Technique(ItemRef)`: targets, range and `Aim` (hit chances, damage) from
  `technique::check_target` and the same `combat::odds` / `preview_damage` the blow uses.
- `front`: `InputEvent::Techniques` (`a`) opens a `Chooser` of `view::techniques` (blocked ones shown
  with the reason, as spells are), then `Aiming` with `Shot::Technique`; status line text in `targeting`.
- `gui`: bind `a`; the chooser is drawn like the spell chooser (`modes.rs`); a `techniques` shot.
- README controls.
- Tests: a `front::Session` flow `a` → pick → aim → perform; a blocked technique cannot be picked;
  `scripts/shots.sh --check` with the new shot.

### 3. Areas: cleave and sweep
- A technique with an `Area` strikes everyone it catches, one `blow` each in `SpawnSeq` order, each its
  own roll; `hits` decides who counts (`Enemies` for both below).
- Extract from `spell::footprint` the part that turns a shape into tiles and creatures, so a spell and a
  technique catch the same creatures for the same shape (shared with `aim`). Spells keep their flight.
- Content: `Cleave.ron` (`Nova { radius: 1 }`, needs Slash, `damage_pct: -25`), `Sweep.ron` (`Cone`,
  reach 2, needs Slash, `damage_pct: -50`). The rusty knife is the only slashing weapon today; a
  proper blade (a sabre) joins the castle's finds so cleaving is worth carrying.
- Tests: cleave hits every adjacent enemy and spares allies; a wall blocks a sweep like a cone spell;
  the preview names everyone the blow will reach.

### 4. Moving bodies: shove and trip
- `Shove { tiles }` payload: on a hit, the target is pushed straight away from the actor until blocked;
  `Command::Shove` moves it, stopping at walls and creatures (no RNG, no damage from the wall yet).
- `Trip`: on a hit, a timed `OffBalance` effect (ADR 0006: `start` + `duration`, through
  `ApplyEffect`) that lowers the target's Dodge in `AttackOdds`. Read from timestamps, ended by
  `ModifyDuration`.
- Content: `Shove.ron` (needs Empty offhand), `Trip.ron` (no needs, `damage_pct: -50`).
- Tests: shove distance, blocked shoves, shoving an ally; off-balance changes the odds and only while
  active; a save mid-effect.

### 5. Creatures perform them
- `techniques:` in a blueprint (inherited unless replaced, `[]` clears, checked at load, spawned
  attached), exactly as `spells:`.
- `TechniqueAi`: before a bump or a step, the first technique `ready` and `check_target` allow that would
  hit its target and none of its own side. In `ai.rs`, next to `SpellAi`.
- Content: goblins shove; the spirit wolf trips (a natural blow, no `needs`). The wolf's is an ability
  in the vision's sense.
- Tests: an AI lunges from two tiles, never into an ally's line; it bumps while recharging; same seed
  gives the same fight.

### 6. Learning techniques
- `Grant::Technique(name)` in `discovery.rs`: a discovery can grant one, attached to the learner.
- Manuals: `Teaches(technique)` beside `Lesson` on an item; `Study` attaches the technique once per kind
  of manual (`Studied`), with the lesson XP if it also has some.
- The player blueprint loses Lunge. Content: "a treatise on the lunge" (`Teaches("Lunge")` plus a small
  Pierce lesson), added to the castle's finds.
- Side panel / skills tab: known techniques listed (`front::text`).
- Tests: studying teaches it once; a second copy teaches nothing; a discovery grants it; it survives a
  dive and a save.

### 7. Styles invent
- `Style.invents: Vec<(u32, BlueprintId)>`: when a followed style's skill reaches the level during a use
  with stakes (inside `skill::train`, where styles already train), the technique is attached with a log
  line ("Fencing teaches you something new: the lunge."). Practice and lessons never invent (they go
  through `skill::gain`), as with discovery.
- The ideas journal (`discovery::ideas`) shows the next invention of the followed style ("your fencing
  is close to something…") once its skill is within a level or two.
- Content: Fencing invents Lunge at 3. A second martial style, **Brawling** (follows Melee and Unarmed;
  discovered from Melee, Unarmed and Dodge), with a principle and inventing Shove, so a style that invents
  is not a one-off. Pick the principle from hooks that exist (`AfterHit`, `AttackOdds`).
- ADR 0019 consequence "Inventions wait for the techniques plan" is replaced by a pointer to 0024.
- Tests: invention fires at the level only with stakes; not from practice; not twice; a style followed
  then set aside keeps what it invented.

## Out of scope

- A stamina or fatigue pool, and techniques that cost health.
- Origins (starting techniques by origin) and legacy technique books (vision: lessons capped at a
  fraction of the predecessor's level); both build on slice 6.
- Ranged techniques (aimed shot, volley) and magic styles inventing spells (ADR 0016).
- Combos or chaining techniques, reactions other than the existing riposte.
- Mouse-only paths: the chooser and cursor take clicks the way spells do, nothing more.

## Open questions for the owner

- Price: is a recharge alone right, or should techniques draw on a stamina pool from the start?
- Should invented techniques work only while their style is followed? (Plan says no.)
- Recharge in turns or ticks scaled by speed? (Plan: ticks, like spell cooldowns, so speed does not
  shorten it.)
