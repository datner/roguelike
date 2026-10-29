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

### 1. Odds with reasons
- `Reason`, `AttackOdds.why`; base and outnumbered named in `odds`; each part's change named after the
  part (the event dispatch reports which handler ran; the change is the difference).
- `game::hit_chance(state, attacker, defender) -> Option<(u32, Vec<Reason>)>` for previews.
- Tests: an even blow's reasons are the base alone; outnumbered adds a named reason; an off-balance
  defender adds one named after the effect; the reasons add up to the chance (before the clamp).

### 2. The log's why
- `Log` entries with an optional why; blows and damage write theirs; `Log::recent_with_why`.
- `front`: `?` (`InputEvent::Why`) expands the bottom strip's lines with their whys for one screen;
  the gui binds `?` and draws the indented lines; README Controls.
- Tests: a miss carries its odds; a `Session` toggles the expanded log on `?` and back on the next key;
  a shot of the expanded strip.

### 3. Previews and hidden things
- The look status on a creature shows "hit 70% (base 75, outnumbered −5)…" when you could attack it;
  the aim status the same for a shot.
- Hidden creatures (grass) are left out of look and aim cycling.
- Tests: the look status names the chance and its reasons; a goblin in grass two tiles off is not
  offered.

### 4. A death that names its chain
- `DeathView` set when the player dies (`command::die`): the killer and the blow, outnumbered count,
  rest budget, place. `game::death(state)`; the death status line in `front` says it.
- Tests: dying to a goblin while outnumbered names both; dying to a fall names the chasm.

## Out of scope

- The one-place bonus sum with caps (needs bonus sources that don't exist yet).
- Why-lines for spells' fizzles and costs beyond what the cast preview already shows.
- Judgement, renown and beliefs on screen (their systems don't exist yet).
