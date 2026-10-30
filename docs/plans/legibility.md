# Plan: legibility

Handoff document. Read `docs/vision.md` (pillar 7, Legibility, Interconnection), ADR 0013 (aim
previews), ADR 0017 (skills), `docs/plans/fights.md` and `game/CLAUDE.md` first.

## Goal

Pillar 7: "Every 'why?' has an answer on screen." The vision's Legibility section asks for numbers
that explain themselves, a log that answers "why?", previews before committing, and a death that
names its chain. The fights plan added rules (outnumbered, telegraphs, grass, sprinting) whose
effects are only partly shown. This plan makes the fight's numbers and outcomes explain themselves,
starting where a player asks most: "why did I miss?", "why did that hit so hard?", "how did I die?".

## Decisions taken for this plan

- **Odds carry their reasons.** `combat::odds` already builds the hit chance in one place: its base
  (accuracy against dodge) and the outnumbered rule are named as they are added, and whatever the
  parts change afterwards (off balance, principles) is named after the part that changed it, read as
  the difference across each part's handler. `AttackOdds.why: Vec<Reason>` (`Reason { what, bp }`).
  Nothing about the roll changes, so no stream moves.
- **The log keeps the why beside the line.** A log entry may carry a why (`Log::push_with`): a blow's
  outcome carries its odds ("hit 85%: base 75, outnumbered +10"); damage carries what armor took.
  Most lines have none. Saves keep only the text (the why is for now, not for later).
- **One key shows the whys**, `?`: the bottom strip expands the latest lines with their whys indented
  under them, until the next key. Keyboard first; no mouse-only path.
- **Previews say the same.** The look cursor on a creature you could hit, and the aim status, show
  the hit chance with its reasons, from the same `odds`.
- **A death names its chain**: when the player dies, `GameState` keeps a `DeathView`: the blow that
  killed (who, with what, how much), how outnumbered you were, the rest budget left on the floor,
  the floor. The death status line and the session say it. Not saved: a dead character's save is
  gone.
- **Hidden is hidden everywhere**: a creature hidden in grass is left out of the look and aim lists
  as well as the drawing (a gap the fights plan left).
- **Bonuses summed under caps is not in this plan.** The vision's one-place sum needs the bonus
  sources (phases, attunement, meals, relics) that do not exist yet; the odds' reasons are the seam
  it will grow from.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first.

### 1. Odds with reasons — done
- `Reason`, `AttackOdds.why`; base and outnumbered named in `odds`; each part's change named after the
  part (the event dispatch reports which handler ran; the change is the difference).
- `game::hit_chance(state, attacker, defender) -> Option<(u32, Vec<Reason>)>` for previews.
- Tests: an even blow's reasons are the base alone; outnumbered adds a named reason; an off-balance
  defender adds one named after the effect; the reasons add up to the chance (before the clamp).
- As built: `EventRegistry::fire_traced` (the same dispatch, reporting each handler's holder and the
  event before and after); `combat::odds_explained` names the base, being outnumbered, each part by
  its `Name` ("reeling") and the clamp ("the limit"), so the reasons always add up to the chance;
  `odds` is it without the reasons. `game::hit_chance(state, at)` gives frontends the player's chance
  on whatever stands there, in percent. `AttackOdds` stays a plain `Copy` event.

### 2. The log's why — done
- `Log` entries with an optional why; blows and damage write theirs; `Log::recent_with_why`.
- `front`: `?` (`InputEvent::Why`) expands the bottom strip's lines with their whys for one screen;
  the gui binds `?` and draws the indented lines; README Controls.
- Tests: a miss carries its odds; a `Session` toggles the expanded log on `?` and back on the next key;
  a shot of the expanded strip.
- As built: `MessageLog::push_with` and `recent_with_why`; whys are never saved and logs compare by
  their messages. Misses, grazes, perfect dodges and deflections carry "75% to hit: accuracy 2
  against dodge 2 75, outnumbered by 1 +10" (`combat::explain`), melee and missiles alike; a hit says
  nothing of its own (its damage line comes later), so it has no why yet. `front::text::log` builds
  the rows, the why under its line. No shot of the expanded strip: a shot can't put a why in the log
  from the window crate; the session test pins the rows.

### 3. Previews and hidden things — done
- The look status on a creature shows "hit 70% (base 75, outnumbered −5)…" when you could attack it;
  the aim status the same for a shot.
- Hidden creatures (grass) are left out of look and aim cycling.
- Tests: the look status names the chance and its reasons; a goblin in grass two tiles off is not
  offered.
- As built: the look status on any creature adds "a blow hits 85% (accuracy 2 against dodge 2 75,
  outnumbered by 1 +10)" (`front::targeting::hit_line`). The aim status was left as it is: a shot's
  preview already says what it would do (ADR 0013), and its odds' reasons are the same function away.
  Grass-hidden creatures are left out of `look_targets`, `inspect` and `aim_targets`.

### 4. A death that names its chain — done
- `DeathView` set when the player dies (`command::die`): the killer and the blow, outnumbered count,
  rest budget, place. `game::death(state)`; the death status line in `front` says it.
- Tests: dying to a goblin while outnumbered names both; dying to a fall names the chasm.
- As built: the damage applier keeps what last hurt the player as it reads ("the goblin's blow",
  "the goblin archer's shot", "bleeding", "the fall into the chasm") with the amount; dying turns it
  into a `DeathView` with how outnumbered you were, the rest budget left and the place. The death
  status reads "You died on turn 812: killed by the goblin's blow (5); outnumbered by 2; 3 rest
  left; The caves, floor 3. Press any key…". Neither is saved.

## Out of scope

- The one-place bonus sum with caps (needs bonus sources that don't exist yet).
- Why-lines for spells' fizzles and costs beyond what the cast preview already shows.
- Judgement, renown and beliefs on screen (their systems don't exist yet).

Exempt: shows other systems' numbers and causes; text only, which the rule does not count.
