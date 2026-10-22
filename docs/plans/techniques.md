# Plan: techniques

Handoff document. Read `docs/vision.md` ("The character", especially Styles and "Techniques and
abilities"), ADR 0014 (spells), ADR 0017 (skills), ADR 0019 (styles) and `game/CLAUDE.md` first.

## Goal

Special actions for the body and the blade: a lunge, a cleave, a shove, a three-cut sword form. Today a
creature fights only by bumping, and styles change how it reacts (a riposte) but never give it anything
new to *do*. Techniques are that missing link: `skills → styles ──invent──> techniques`. They are
learned from a manual, invented by a followed style, or, for a creature, simply known from its blueprint
(an ability).

## Decisions taken for this plan

- **A technique is a blueprint entity attached to its holder**, like a spell (ADR 0014): in use, saved
  and despawned with them. `Technique { domain, reach, recharge, stamina, requires }` says how it is
  aimed, what it asks and what it costs; payload parts say what it does.
- **It shares the spell's effect half, not its source or payment.** Areas reuse `Area` and `area::tiles`;
  aiming reuses `aim` with a new `Shot::Technique`. No discipline, no `CheckCast`/`PayCost`, no scroll.
- **Price: a turn, a recharge, and stamina.** `cost::PERFORM`, then `Recharging(Timed)` on the
  technique (the type spells use, read from timestamps, ADR 0006), counted in ticks so speed does not
  shorten it. A technique whose recharge should follow its holder's speed gets a flag when one needs it.
  **Stamina is real but dormant**: every technique has `stamina: 0`, and a `Stamina` pool exists as a
  part (computed on read like `Mana`, sharing its math) that no blueprint has yet and no view shows.
  Turning stamina on is a data change: give techniques a cost and creatures a `Stamina` part. A cost above
  0 with no pool is refused ("You have no stamina for that."), so a half-done data change fails loudly
  in tests, not silently in play.
- **Requirements are an open list**, `requires: Vec<Need>`, each checked by one function with its own
  refusal message. Weapon gating is the common case, not the only one. First variants: `Wielding(skills)`
  (the blow uses these skills: a thrusting weapon, an empty off hand), `Skill(name, level)`,
  `Following(style)`; `After(..)` comes with combos. A new kind of requirement is a variant and a match
  arm, never a special case in `perform`.
- **A technique's blow is a blow, unless it says otherwise.** It strikes through the same
  `combat::odds` / `combat::resolve` / roll as a bump, so grazes, dodges, deflection and on-hit parts
  apply, and it trains what it uses through `skill::after_blow`, which also trains the followed style when
  the use follows it (ADR 0019). A technique that is something else (a qi strike) names the skills it
  uses instead with `Uses { skills }`, which then decide both its level and what it trains.
- **Styles change techniques, per technique.** A technique may list `bonuses` that apply while a given
  style is followed: higher numbers, an extra rider (shove, trip), lower recharge or stamina. Whether a
  technique *only* works under its style is its `requires` (`Following`), also per technique.
- **Combos are data, built on the two above.** A technique can require or reward having just performed
  another (`After { technique, within }`, read from a timestamp on the actor, ADR 0006). A form like the
  tri-sword is three techniques in a `group`: performed in order within the window, each step gets the
  bonus the next lists and extra XP. Whether a group shows as one entry that turns into its next step, or
  as three entries, is the group's `display` choice, so both shapes the owner described can be content.
- **Abilities need no new mechanism**: a creature's `techniques:` list in its blueprint (like `spells:`).
  What distinguishes an ability (it comes from the body or origin) waits for origins.
- **Key:** `a` (act) opens the technique list, then the aim cursor, like `c` for spells. Free in `gui`.
- New ADR **0024 — Techniques**, written in slice 1 and amended per slice.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first. Every new
component: blueprint `ComponentDef`, `SavedComponent` + round-trip test, `SAVE_VERSION` bump.

### 1. A technique, end to end in `game`: the lunge — done
- `entity.rs`: `Technique { domain, reach, recharge, stamina, requires: Vec<Need> }` (saved by name like
  `Style`, since `Need` holds ids), `Blow { accuracy, damage_pct }`, `Dash { tiles }`, `Stamina` (pool,
  same shape as `Mana`; extract the shared computed-on-read math rather than copying it).
- `command.rs`: extract the body of `attack` into `combat::blow(state, attacker, target, &BlowMods)`; a
  bump is `blow` with no mods, so existing combat tests pin it.
- `technique.rs` (new): `ready(state, actor, technique) -> Result<Ready, String>` (recharging, each
  `Need`, stamina) and `check_target(state, actor, ready, at)` (an enemy in reach, on a straight or
  diagonal line, the tiles between open and empty when it dashes). One shared check, as
  `spell::ready_cast`: `reject_input`, `aim` and AI all call it. `Need`: `Wielding`, `Skill`.
- `PlayerAction::Perform { technique: ItemRef, at }` → `Command::Perform { actor, technique, at }`
  (`cost::PERFORM = STANDARD`): pay stamina (a no-op at 0), dash along the line (a plain position
  change, no energy, no RNG), `blow` with the `Blow` mods, then `Command::Recharge`. Logs "You lunge at
  the goblin." through `Subject`.
- Content: `data/entities/Lunge.ron` (reach 2, dash 1, `Blow(accuracy: 2, damage_pct: 25)`, requires
  `Wielding(["Pierce"])`, recharge 5 turns, stamina 0). Given to the player blueprint for now so it can
  be played; slice 8 takes it away.
- `view::techniques(state) -> Vec<TechniqueView>` (name, ready or why not, turns until recharged; no
  stamina while every cost is 0).
- Tests: lunge hits from two tiles and lands adjacent; refused when recharging, out of reach, off-line,
  blocked, with no target, with a slashing weapon, under a `Skill` need (each with its message); a
  stamina cost is paid from a pool and refused without one or when short; the recharge expires on
  time; a seeded lunge draws the combat stream like a bump; save round-trip mid-recharge.
- ADR 0024.

### 2. From the keyboard, with a preview — done
- `aim.rs`: `Shot::Technique(ItemRef)`: targets, range and `Aim` (hit chances, damage) from
  `technique::check_target` and the same `combat::odds` / `preview_damage` the blow uses.
- `front`: `InputEvent::Techniques` (`a`) opens a `Chooser` of `view::techniques` (blocked ones shown
  with the reason, as spells are), then `Aiming` with `Shot::Technique`; status line text in `targeting`.
- `gui`: bind `a`; the chooser is drawn like the spell chooser (`modes.rs`); a `techniques` shot.
- README controls.
- Tests: a `front::Session` flow `a` → pick → aim → perform; a blocked technique cannot be picked;
  `scripts/shots.sh --check` with the new shot.

### 3. Areas: cleave and sweep — done
- A technique with an `Area` strikes everyone it catches, one `blow` each in `SpawnSeq` order, each its
  own roll; `hits` decides who counts (`Enemies` for both below).
- Extract from `spell::footprint` the part that turns a shape into tiles and creatures, so a spell and a
  technique catch the same creatures for the same shape (shared with `aim`). Spells keep their flight.
- Content: `Cleave.ron` (`Nova { radius: 1 }`, `Wielding(["Slash"])`, `damage_pct: -25`), `Sweep.ron`
  (`Cone`, reach 2, `Wielding(["Slash"])`, `damage_pct: -50`). The rusty knife is the only slashing
  weapon today; a sabre joins the castle's finds so cleaving is worth carrying.
- Tests: cleave hits every adjacent enemy and spares allies; a wall blocks a sweep like a cone spell;
  the preview names everyone the blow will reach.

### 4. Riders and other kinds of blow — done
- `riders: Vec<Rider>` on a technique, applied on a full hit (as built: grazes never carry riders;
  `Command::Ride` checks the target lived, since its `Die` queues after the rider). A rider is data, so a style bonus can add one (slice 5). First riders:
  - `Shove { tiles }`: pushed straight away from the actor until blocked; `Command::Shove` moves it,
    stopping at walls and creatures (no RNG, no wall damage yet).
  - `Effect(blueprint)` (as built, instead of a `Trip` rider): any timed effect through
    `ApplyEffect`; the trip's is `OffBalance.ron`, an `OffBalance` part that raises the hit chance
    against its target and takes away its perfect dodge while attached.
- `Uses { skills }`: the blow's level and its training come from these skills instead of the weapon's
  (`skill::melee`), for techniques that are not plain weapon work. Tested with an in-test qi strike; no
  content needs it yet.
- Content: `Shove.ron` (`Wielding(["Empty offhand"])`, rider only), `Trip.ron` (no needs,
  `damage_pct: -50`, trip rider).
- Tests: shove distance, blocked shoves, shoving an ally; off-balance changes the odds and only while
  active; a save mid-effect; `Uses` changes the level and the skills trained, and the weapon's skills
  are not trained.

### 5. Styles shape techniques — done
- `Need::Following(style)`: the technique works only while that style is followed.
- `bonuses: Vec<StyleBonus { style, accuracy, damage_pct, recharge_pct, stamina_pct, riders }>` on a
  technique: applied in `technique::ready` (prices) and the blow (numbers, riders) while that style is
  followed, so the preview shows them. Bonuses may switch on at a level of the style's skill
  (`from_level`), like principles.
- Content: Fencing makes the lunge recharge 40% faster from its level 2 and adds the off-balance
  rider from its level 5. A style is named by its skill's name (`Following("Fencing")`).
- Tests: the bonus applies only while following, and only from its level; the preview and the
  performance agree; a `Following` technique is refused, with its message, after setting the style
  aside.

### 6. Combos: the tri-sword — done
- `Performed { technique, at: Tick }` on the actor (the last technique and when), written by `Perform`.
- As built: a `combo: { group, step, within, accuracy, damage_pct, riders, xp_pct, display }` entry
  instead of naming the technique before, so no technique refers to another: a step continues a run
  when the actor's `Performed` is the previous step of its group, within `within` ticks.
  `Need::Continuing` makes a step only follow the one before; without it, a step can open a run and
  gets its bonus only when continuing. Each blow of a run teaches `xp_pct` more per step already in it
  (`skill::after_blow`'s extra, stakes still required).
- `group: Option<Group { name, display }>`: `display: Steps` lists every technique of the group, `Turns`
  shows one entry that is whichever step is next (the first after the window passes). `view::techniques`
  folds a `Turns` group into one row; `front` needs nothing new.
- Content: the tri-sword, three sword techniques (horizontal slash, vertical slash, stab), `Turns`
  display, each `After` the one before for its bonus, granted together by whatever teaches the form
  (slice 8: a manual can teach several).
- Tests: the second step is boosted only within the window and only after the first; a missed window
  restarts the form; XP rises along a run and resets after it; a `Turns` group shows as one row that
  advances; same inputs give the same combo.

### 7. Creatures perform them — done
- `techniques:` in a blueprint (built in slice 1, so the player could start with the lunge).
- `TechniqueAi`: before a bump or a step, the first technique `ready` and `check_target` allow that would
  hit its target and none of its own side. In `ai.rs`, next to `SpellAi`.
- Content: goblins shove; the spirit wolf trips (a natural blow, no needs), an ability in the vision's
  sense.
- Tests: an AI lunges from two tiles, never into an ally's line; it bumps while recharging; same seed
  gives the same fight.

### 8. Learning techniques
- `Grant::Technique(name)` in `discovery.rs`: a discovery can grant one, attached to the learner.
- Manuals: `Teaches(Vec<technique>)` beside `Lesson` on an item; `Study` attaches them once per kind of
  manual (`Studied`), with the lesson XP if it also has some.
- The player blueprint loses Lunge. Content: "a treatise on the lunge" (`Teaches(["Lunge"])` and a small
  Pierce lesson) and a tri-sword scroll, added to the castle's finds.
- Side panel / skills tab: known techniques listed (`front::text`).
- Tests: studying teaches once; a second copy teaches nothing; a discovery grants one; they survive a
  dive and a save.

### 9. Styles invent
- `Style.invents: Vec<(u32, BlueprintId)>`: when a followed style's skill reaches the level during a use
  with stakes (inside `skill::train`, where styles already train), the technique is attached with a log
  line ("Fencing teaches you something new: the lunge."). Practice and lessons never invent (they go
  through `skill::gain`), as with discovery.
- The ideas journal (`discovery::ideas`) shows the followed style's next invention ("your fencing is
  close to something…") once its skill is within a level or two.
- Content: Fencing invents Lunge at 3. A second martial style, **Brawling** (follows Melee and Unarmed;
  discovered from Melee, Unarmed and Dodge), with a principle picked from hooks that exist (`AfterHit`,
  `AttackOdds`) and inventing Shove, so a style that invents is not a one-off.
- ADR 0019's consequence "Inventions wait for the techniques plan" is replaced by a pointer to 0024.
- Tests: invention fires at the level only with stakes; not from practice; not twice; a style followed
  then set aside keeps what it invented.

## Out of scope

- Showing stamina, regaining it beyond the pool's own regeneration, or any non-zero stamina cost in
  content. Techniques that cost health.
- Origins (starting techniques by origin) and legacy technique books (vision: lessons capped at a
  fraction of the predecessor's level); both build on slice 8.
- Ranged techniques (aimed shot, volley) and magic styles inventing spells (ADR 0016).
- Reactions other than the existing riposte.
- Mouse-only paths: the chooser and cursor take clicks the way spells do, nothing more.
