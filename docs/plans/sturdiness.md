# Plan: sturdiness (health growth and recovery)

Handoff document. Read `docs/vision.md` (pillars 1, 2 and 4, Skills, Fights: attrition),
`docs/plans/rest.md`, ADR 0017 (skills) and `game/CLAUDE.md` first.

## Goal

Health is 20 for everyone and never grows, so a level-10 sword build is a glass cannon: the bench's
strong builds die to plain goblins and rats. Staying power should grow, and stay a **composite** of
layers that grow from different places, so builds differ in how they last: health, armor, Toughness,
resistances, and avoidance (Dodge, Poise, Deflection). Armor, resistances and avoidance exist; this
plan adds the health layers and the recovery that refills them.

## Decisions taken for this plan

- **Origins start at different health**, written in each origin's blueprint (the pit fighter 26, the
  duelist 20, the wanderer 20, the hedge mage 16). Data, not a formula: stats are the same for every
  origin today.
- **Toughness is a skill trained by being hurt in a fight** (Defence domain). Damage dealt by an enemy
  (a blow, a shot, a spell, an effect it caused) trains it through `skill::train`, so stakes,
  familiarity and novelty apply: a new kind of foe teaches more than the hundredth goblin. Practice
  never trains it. A fall is not a fight.
- **Toughness pays off in max health**: a `Hardiness { per_level }` part (2 on `Player`) makes max
  health the blueprint's base plus `per_level` × Toughness level. The base is kept in `BaseHealth`;
  max health is refreshed whenever Toughness trains, and the gain is healed (growing tougher doesn't
  leave you hurt). One place pays it off (`health::refresh`), as the skills ADR asks.
- **Regeneration heals a share of max health**, not a point: `Regeneration { share }` is a percent of
  max per tick (5), so a rest takes as long at 40 health as at 20; the floor budget was already a
  percent of max.
- **Recovery is a skill trained by healing real wounds** (Defence). Health lost to an enemy on a floor
  is owed (`Wounds`); each point regeneration pays back trains Recovery through `skill::gain`. Healing
  from idle (nothing owed) teaches nothing, so resting can't be farmed. Recovery shortens the
  regeneration interval (10% per level) and deepens the floor budget (+10 percentage points per
  level): careful play gets its own reward.
- **Legible enough**: Toughness is part of max health, not a stat of its own; the skills tab shows
  the level. A breakdown of max health belongs to the vision's numbers-explain-themselves work.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first.

### 1. Origins and Toughness — done
- Origin blueprints' `Health`; `Toughness` in `data/skills.ron`; `Hardiness`, `BaseHealth`,
  `health::refresh`; training on damage from an enemy; the bench's `dress` refreshes after setting
  skills; the sheet's health line.
- Saves (`Hardiness`, `BaseHealth`), `SAVE_VERSION`.
- Tests: the origins start at their health; a goblin's blow trains Toughness and a fall does not;
  a level raises max health and heals the gain; the sheet names the bonus; a build at Toughness 10
  starts at 40.
- As built: `game/src/health.rs`. Max health is the only number: no separate Toughness figure on
  the sheet (the owner's call). Wounds are counted here already (slice 2 repays them). Saves v56.

### 2. Regeneration by share, and Recovery — done
- `Regeneration.share`; `Wounds` (owed, per floor); Recovery in `data/skills.ron`, trained per point
  repaid; its interval and budget effects in `rest`.
- Tests: a tick heals 5% of max; with nothing owed healing trains nothing; repaying wounds trains
  Recovery; a higher Recovery heals sooner and has a deeper budget.
- As built: a tick heals 5% of max (at least a point) as far as the budget allows; each point that
  repays a wound gives Recovery 2 XP. Wounds are forgotten on arriving somewhere new. Recovery
  divides the interval by 1 + level/10 and adds 10 points per level to the floor budget's percent.

### 3. The bench — done
- Level-10 builds gain Toughness and Recovery at 10 (a veteran is tougher, not only sharper); the
  report regenerated; this plan's as-built note says what moved.
- As built: two new builds, `sabre veteran` and `tri-sword veteran`, rather than changing the level-10
  ones: the doll tables compare builds by the XP they took, and Toughness would skew that. In the
  caves (20 runs each), reaching floor 6: sabre veteran 18, tri-sword veteran 19; the plain level-10
  tri-sword 19 (from 5: it grows Toughness as it is hurt) and sabre 3 (from 0); the pit fighter, at 26
  health, 6 (from 0). The hedge mage, now at 16, still 19.

## Out of scope

- Health from stats (stats don't grow and are the same for every origin today).
- Field dressings, meals and healers (crafting and camp).
- Hardiness discoveries (a proficiency from Toughness with another skill): a later layer.
