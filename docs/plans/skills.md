# Plan: skills, discovery and styles

Handoff document. Read `CLAUDE.md`, `game/CLAUDE.md`, `docs/vision.md` (the why: pillars, how the
character systems link), `docs/decisions/0004-data-driven-blueprints.md`, `0011-stats-and-weapons.md` and
`0013-ranged-attacks.md` first. This plan is self-contained beyond those.

## Goal

Build the character progression of `docs/vision.md` up to styles: skills that grow by use, discovery
from skill combinations, proficiencies, and the first style. Techniques, life skills, the camp, phases,
origins and legacy are separate plans; this one stops where they start.

## Ground rules (from the project; do not break)

- `game` never depends on a terminal crate. `term` talks to it through `PlayerAction` and plain views.
- All world mutation goes through `Command` / `apply_commands` (ADR 0002). Event handlers get
  `&World` and emit commands; they never mutate.
- Determinism (ADR 0005): no `HashMap`/`HashSet`, no clock. Skill gain draws no randomness.
- Content is data (ADR 0004): skills, a tool's skills, starting skills, discoveries and styles are RON.
- No `unwrap`/`expect` in library code. Tests sit next to the code.
- Every slice ends with `scripts/check.sh` green. Never run `term`; test UI with `TestBackend`.
  Do not commit unless asked. Commits are conventional and scoped by crate.
- Recipes: `add-part` (components, blueprints, save), `add-command`, `add-term-ui`, `add-spell` (slice 5).

## Where things stand

- Creatures have `Stats` (STR/AGI/INT/WIL, 16 average, `Stats::modifier`). No levels, no XP.
- Melee: `combat::strike` picks the first equipped `Weapon` by body-part order (else natural `Melee`) and
  builds `Strike { min, max, bonus, types, weapon }`; `bonus` is the weapon's stat modifier.
  `roll_strike` rolls damage only: attacks never miss yet, melee or ranged (ADR 0013). Slices 1b and 4
  change that.
- `preview_damage` and `aim` read the same `Strike`, so anything folded into it is previewed for free.
- Spells have a `School`, a shape and a fizzle chance (`CastRisk`, basis points).
- Hands: `Equippable(part: Hand)` only. No two-handed weapons; only one weapon strikes.
- `DamageType`: Slashing, Blunt, Piercing, Fire, Cold, Shock.

## Design (decided)

1. **Skills are flat, data-defined tags.** `data/skills.ron` lists every skill with its domain and
   weight: `(name: "Dagger", domain: Martial, weight: 3)`. Load assigns each a `SkillId` (index in file
   order, like `BlueprintId`); saves store names. Every skill named in data is checked at load.
2. **An action's skill set** comes from:
   - the action kind, in code: Melee, Thrown, Archery, Magic;
   - the tool, in data: a `Trains([..])` part on the item, spell or creature (natural attacks), e.g. the
     rusty knife has `Trains(["Dagger"])`;
   - the tool's damage types, in code: Piercing → Pierce, Slashing → Slash, Blunt → Blunt, so data does
     not repeat what `Weapon.types` already says;
   - the circumstance, in code: One-handed, Empty offhand (slice 3).

   `skill::used(world, actor, action) -> SkillSet` collects them (sorted, deduplicated `Vec<SkillId>`).
   A `GatherSkills` event that lets parts add skills is the extension point for proficiencies and styles;
   add it when the first one needs it.
3. **Effective level is a weighted average**: `Σ weight × level / Σ weight`, integer, rounded down.
   Specific skills weigh more (Melee 1, Pierce 2, Dagger 3). A sum would make the most-tagged tool best.
4. **Storage: XP only, levels derived.** `Skills(Vec<(SkillId, u32)>)` on the creature, sorted by id.
   `skill::level(xp)` is pure, so the curve can change without a save migration. Missing = level 0.
5. **Gain by use** through `Command::Train { actor, skills, subject, xp }`. Every skill in the set gets
   the XP. A use counts only if it passes its domain's test (`docs/vision.md`): combat and magic use
   *stakes* — it was aimed at a creature of an enemy side (`side::enemies`). A miss teaches too, but
   less; an unlikely hit teaches more (slice 1b).
6. **Repetition decays.** `Familiarity(Vec<(SkillId, BlueprintId, u32)>)` counts uses per skill and
   subject kind (the target's blueprint). XP = `base × K / (K + uses)`. Your 50th goblin teaches less than
   your first troll; variety restores the rate.
7. **Attacks can miss** (owner's decision; changes ADR 0013). Combat skill raises hit chance and adds a
   damage bonus to `Strike.bonus` next to the stat modifier. Hit chance = attacker's effective level and
   stat modifier against the defender's Dodge level and Agility modifier, clamped, in basis points, rolled
   from the combat stream. Preview and `aim` show it, from the same function the attack uses.
   **A miss can still graze.** An attack resolves to *hit*, *graze*, *deflected*, *miss* or *perfect
   dodge*:
   1. Hit roll. Success → hit.
   2. Otherwise the defender's perfect-dodge chance (the body: Poise): success → perfect dodge, and no
      graze is calculated at all.
   3. Otherwise the graze roll, the weapon's graze chance. Failure → miss.
   4. A graze meets the defender's deflect chance (the gear: a buckler, a pauldron, plus the Deflection
      skill): success → deflected, nothing gets through; else → graze.

   Each roll is drawn only when its chance is above 0 and below certain, so creatures without the
   relevant parts leave the combat stream untouched. A graze deals half the rolled damage (before armor)
   and fires `AfterHit` with `graze: true`; an `OnHit` entry applies on a graze only if it says
   `on_graze: true` (a poisoned needle does, a bleeding cut does not).
   The chances are gathered by an `AttackOdds { hit_bp, perfect_bp, graze_bp, deflect_bp }` event fired
   at the attacker, then the defender, so weapons, gear, skills, proficiencies, styles and abilities adjust
   them in handlers (an ability "every dodge is perfect" sets `perfect_bp` to certain). Preview and `aim`
   run the same event, as `spell::inspect_cast` does for casts.
   Perfect dodge and deflection are both *outcomes*, with their own event and log line, so other things
   react to them: a Fencing riposte on a perfect dodge, a shield bash or wear on the deflecting item on a
   deflection. A perfect dodge teaches more. Deflection names the item that turned the blow
   (`Deflected { attacker, defender, by }`). Blocking full hits with a shield is a separate, later thing.
8. **Only the player learns** (owner's decision). Monsters have fixed `Skills` from their blueprint and
   never gain XP: they do not live long or act consistently enough for growth to mean anything. The
   `Learner` part marks the player.
9. **Feedback.** A log line when a skill gains a level ("Your dagger skill rises to 3."), never per hit.

Defaults (in data or one `const` each, so tuning needs no code change): levels 0 to 20; level `n` needs
`25 × n²` XP in total; base 5 XP per counted use; decay `K` = 10; bonus = effective level / 4; hit chance
75% at equal footing, ±4% per point of difference, clamped to 5–95%; weapon graze chance 30% unless the
weapon says otherwise; a miss or graze teaches half; a hit teaches `base × (1 + miss chance)`.

## Slices

Each slice: tests first where practical, then code, then docs (ADR, `README` if user-visible,
`scripts/map.sh`), then `scripts/check.sh`.

### 1. Melee skills, end to end in `game`
- `data/skills.ron`: Melee, Unarmed, Dagger, Pierce, Slash, Blunt. Loaded with the blueprints into a
  `SkillBook` (names, domains, weights) held by `GameState`.
- New module `skill.rs`: `SkillId`, `SkillSet`, `Domain`, `level`, `effective`, `used(.., Action::Melee)`.
- Parts: `Skills`, `Trains`, `Learner`, `Familiarity`. Blueprint registration (names checked at load) and
  save (by name), `SAVE_VERSION` bump with a round-trip test.
- `combat::strike` adds the skill bonus; `Command::Train` after a melee hit on an enemy, with decay;
  level-up log line through `grammar`.
- Content: `RustyKnife` trains Dagger; natural attacks train Unarmed; `Player.ron` gets `Learner`;
  `Goblin.ron` a fixed Dagger level.
- Tests: weighted average; level curve; damage types map to skills; XP to every skill used, only on enemy
  hits; decay per subject kind; non-learners never change; preview equals the dealt bonus; an unknown
  skill name fails load.
- ADR 0017 (skills): flat tags, weighted average, XP-only storage, deterministic gain, decay.

### 1b. Melee can miss, graze, be deflected or be perfectly dodged
- `AttackOdds` event and `combat::odds(world, attacker, defender) -> Odds { hit_bp, perfect_bp, graze_bp,
  deflect_bp }`, pure; `prio.rs` priorities for its handlers. Base hit chance from skills and stats as in
  decision 7.
- `combat::resolve` rolls hit → perfect dodge → graze → deflect from the combat stream, drawing only what
  is needed, and returns `Outcome::{Hit, Graze, Deflected(item), Miss, PerfectDodge}`. Log lines for each
  ("Your knife grazes the goblin.", "The goblin dodges perfectly.", "Your buckler turns the goblin's
  knife."). Every outcome costs the same energy.
- Parts: `Graze(bp)` on weapons (default when absent, see Defaults); `Deflect(bp)` on equipment (the item
  with the highest chance is the one that turns the blow; ties by body-part order); `OnHitEntry.on_graze`
  (default false); `AfterHit.graze`. `PerfectDodge` and `Deflected` events for later reactions, with no
  handlers yet. Content: a buckler with `Deflect`.
- Skills (domain Defence): Dodge (lowers `hit_bp`), Poise (raises `perfect_bp`; 0 at level 0), Deflection
  (raises `deflect_bp`, only while something equipped has `Deflect`). Trained when an enemy attacks you,
  whatever the outcome; Deflection only when you have something to deflect with. A perfect dodge teaches
  the defender more, up to double the less likely it was (`× (2 − perfect chance)`).
- `preview_damage` / the side panel show hit and graze chances next to the damage range.
- Tests: the clamp; equal footing; each defence skill moves its own chance; a certain perfect dodge never
  grazes or deflects; no `Deflect` gear means no deflect roll; draw counts per outcome (a hit draws once;
  nothing is drawn for a chance of 0); graze halves damage; `on_graze` gates effects; the deflecting item
  is named; preview equals the rolled odds. ADR 0013 gets a note (melee now
  resolves to four outcomes; ranged in slice 4).

### 2. Seeing skills
- `view::skills(state) -> Vec<SkillView { name, domain, level, xp, next }>`, skills with XP or a level.
- `term`: a Skills view in the overlay (beside inventory and equipment), grouped by domain; item
  inspection lists what an item trains. Follow `add-term-ui`. README Controls if a key is added.

### 3. Circumstance skills
- `used` adds One-handed when one hand holds the striking weapon, Empty offhand when every other hand is
  empty, from `equipment::equipped`. Two-handed and dual wielding wait for a grip model and the
  dual-wielding proficiency; note it in ADR 0017.

### 4. Ranged and thrown
- `Action::Thrown` (Thrown + the missile's `Trains` and damage types) and `Action::Fired` (Archery + the
  launcher's `Trains`; `ShortBow`: Bow). Bonus into `fire_strike` / `throw_strike`.
- Missiles resolve with the same `odds` / `resolve` from 1b. The flight path stays RNG-free; only the
  outcome against the first creature in line is rolled. A graze or a deflection stops the missile like a hit. Decide in
  ADR 0013 whether a miss or perfect dodge flies on to the next creature or comes to rest; `aim` shows
  the chances.

### 5. Magic
- `Action::Cast`: Magic + the school + the shape (Bolt, Burst, Cone, Beam, Nova, Chain, Rune) + any
  `Trains` on the spell. Discipline skills (Channeling, Hemomancy, Scroll lore) are trained by paying for a
  cast that counts.
- Effects: school → lower fizzle, through a `CastRisk` handler at its own `prio.rs` priority, so
  `spell::cast` does not learn about skills. Disciplines → cost or regen, in their own handlers.
- XP when the cast catches an enemy. A scroll trains what the spell it holds would.

### 6. Discovery and proficiencies
- `data/discoveries.ron`: `(name, requires: [Skill("Archery", 8), ...], grants: Proficiency("..."))`.
  Requirements over skills now; over styles from slice 7.
- After a `Train` that counted (stakes), check the learner's unmet discoveries in file order; a met one
  emits `Command::Discover` with a log line. Practice (slice 8) never triggers it.
- A proficiency is a blueprint attached to the creature (`Attachments`), so its parts take part in events.
  First content: one proficiency whose effect fits existing hooks.
- `Discovered(Vec<DiscoveryId>)` saved by name. Journal: `view::ideas` lists discoveries with at least
  half their requirements met, showing the met ones.
- ADR 0018 (discovery).

### 7. Styles
- A style is a blueprint entity held by the creature: known styles inert (`Inventory`-like list), the
  active one in `Attachments` so its handlers fire (`holding.rs`). `Style { domains: [Martial, ..] }`;
  one active per domain, a hybrid fills all its domains. `Command::AdoptStyle` costs an action.
- Style XP: the style is a skill in `skills.ron` (domain Style), trained when an action counts and its
  skill set includes the style's `requires` skills.
- Principles: parts with a `from_level` checked against the style's level; below it they do nothing. A
  fresh style is inert by construction.
- First content: Fencing (Melee + Pierce + One-handed + Empty offhand), one principle that changes an
  action, not just a number. Discovery requirements may now name styles.
- ADR 0019 (styles). Inventions (techniques) wait for the techniques plan.

### 8. Lessons and practice
- `Lesson { action, grants: [(skill, xp)] }` part on things; taking one is recorded per kind in the same
  list as novelty, so it pays once. First content needs no new system (a manual item; studying a rune).
- `PlayerAction::Practice`: repeats like resting until the ceiling or an interruption; the ceiling is a
  fraction of the skill's best level reached through counted use, or a low fixed number if never used.

### 9. Lore
- Beast lore and Arcana trained by novelty (first sight, first use). Thresholds unlock lines in an inspect
  view (creature HP and resistances, spell numbers). Needs an inspect view if `term` has none.

## Open questions for the owner

- Level range and curve; the defaults above are a starting point.
- First proficiency and Fencing's first principle: pick from what existing hooks allow.

## Definition of done per slice

`scripts/check.sh` passes; new parts are registered in blueprint and save with a `SAVE_VERSION` bump and
a round-trip test; the slice's ADR is written or updated; `docs/MAP.md` regenerated; a short report
listing what changed, what was left out, and follow-ups.

## As built

- **1** (ADR 0017): `skill.rs` (`SkillBook` inside `Blueprints`, loaded by `Blueprints::load(data_dir)`;
  `Core` skills; `level`, `effective`, `melee`, `train`), parts `Skills` (levels in RON, XP at runtime),
  `Trains`, `Learner`, `Familiarity` (keyed by the subject's `Name`). Training is a call from
  `command::attack`, not a `Command`. `Strike.level` carries the effective level. `SAVE_VERSION` 19.
- **1b** (ADR 0017, amendment to 0013): `AttackOdds` / `combat::odds` / `combat::resolve` /
  `Outcome`; `Graze`, `Deflect` parts; `OnHitEntry.on_graze`; `DamageCause::Melee.graze`,
  `AfterHit.graze`; `PerfectDodge` and `Deflected` events (no handlers); skills Dodge, Poise,
  Deflection; `Strike.accuracy`; `skill::after_blow`; `Buckler.ron`; test-only `SureHits`.
  `SAVE_VERSION` 20. Not shown in `term` yet: there is no melee target view to show odds in.
- **2**: `view::skills` → `SkillView { name, domain, level, xp, next }`; `ItemView.trains` (what the
  item `Trains` plus its weapon's damage-kind skills, `skill::of_item`); `InputEvent::Skills` / `s`;
  overlay `Tab::Skills` (grouped by domain; `Tab` cycles inventory → equipment → skills); item rows
  say "trains …".
