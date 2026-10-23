# Plan: bow dancing

Handoff document. Read `docs/plans/techniques.md`, ADR 0013 (ranged), ADR 0017 (skills), ADR 0019
(styles), ADR 0024 (techniques) and `game/CLAUDE.md` first.

## Goal

A martial style for fighting up close with a short bow in hand: the legs do the close work (kicks that
push and trip) and the bow finishes at arm's length. Today the bow has no weakness up close, so such a
style would fix nothing; first, shooting next to an enemy gets harder for everyone, then the style
answers that. A longbow, warbow or crossbow is too big to dance with; only a short bow qualifies.

## Decisions taken for this plan

- **Point blank is hard for everyone.** Firing a launcher while an enemy stands next to the shooter
  costs `POINT_BLANK` accuracy (start at 4, the size of a few skill levels). It is worked out in
  `ranged::ready_fire`, which the command, `aim` and AI share, so the preview shows the lower chance.
  Thrown things are not affected (a knife thrown up close is fine). No creature shoots yet, so in play
  this only touches the player today; it is a rule, not a player special case.
- **"Short bow" is a skill tag.** A new skill with `weight: 0`: it never counts toward a use's level
  (`skill::level` already skips weightless skills in its average) but appears in what a use uses, so
  `Need::Wielding(["Short bow"])` and a style's `follows` can name it. The short bow `Trains(["Bow",
  "Short bow"])`; a longbow later trains only `"Bow"`. A weightless skill gains XP like any other; it
  is not shown in the skills panel.
- **`Wielding` reads what is in hand, not what the blow uses.** Today it checks the blow's skills; a
  kick's blow is the body's, so a kick that needs a short bow in hand would never qualify. `Wielding`
  checks the wielded weapon's skills (`skill::melee` of the default strike); for every existing
  technique the two are the same.
- **A kick is an unarmed blow with the hands full.** `Blow { unarmed: true }` strikes with the
  creature's natural `Melee`, whatever it wields, and trains what an unarmed blow trains. It needs feet:
  a new `Need::Part(PartKind)` ("Your push kick needs feet.").
- **Kicks follow the style through data.** Kicks list `uses: ["Melee", "Unarmed", "Short bow"]`; the
  tag is weightless, so the level is the unarmed one, and the use carries the tag the style follows.
  No new rule about which uses follow a style.
- **A technique can shoot.** A `Shot { accuracy, damage_pct }` payload fires the next arrow through
  `ranged::ready_fire` and the same launch a plain shot uses (flight, odds, breaking, training), with
  the bonuses added. Its reach is the technique's `reach`. `Dash` gains `away: bool` to step back from
  the target before acting.
- **The style:** "Bow dancing" (skill and style; renaming it is a data change). Follows `["Short
  bow"]`, so bow shots, bow bashes and kicks all train it. Discovered from Archery 3 and Unarmed 2.
- Amendments, not new ADRs: ADR 0013 (point blank), ADR 0024 (unarmed blows, `Part`, `Wielding` reads
  the hand, `Shot`, `Dash` away).

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first. Every changed
component: blueprint `ComponentDef`, `SavedComponent` + round-trip test, `SAVE_VERSION` bump.

### 1. Point blank
- `ranged::point_blank(state, actor) -> bool`: an enemy (`side::enemies`) on any of the 8 tiles around
  the shooter. `ready_fire` lowers `strike.accuracy` by `POINT_BLANK` when it holds.
- `front/targeting.rs`: the aim status says "point blank" when the penalty applies, from a flag on
  `aim`'s result (never worked out in `front`).
- ADR 0013 amendment.
- Tests: the hit chance drops with an adjacent enemy and not with an adjacent ally or a distant enemy;
  the preview and the shot agree; throwing is unaffected; seeded shots draw the combat stream as before.

### 2. The short bow tag
- `data/skills.ron`: `(name: "Short bow", domain: Martial, weight: 0)`. `ShortBow.ron` trains both.
- The skills panel (`view`/`front::text`) leaves out weightless skills.
- Tests: the tag changes no level (a shot's hit chance is the same with and without it); a short-bow
  shot and bash train it; it is not listed in the panel.

### 3. Kicks
- `Blow { accuracy, damage_pct, unarmed }`: `BlowMods.unarmed` makes `combat::blow_strike` skip the
  wielded weapon for the natural `Melee`. `aim` previews it through the same function.
- `Need::Wielding` checks the hand (see decisions); `Need::Part(PartKind)` checks the `Body`.
- Content: `PushKick.ron` (reach 1, unarmed, `damage_pct: -25`, `Shove { tiles: 1 }` rider, recharge 3
  turns) and `SweepKick.ron` (reach 1, unarmed, `damage_pct: -50`, off-balance rider, recharge 5 turns),
  both `Wielding(["Short bow"])`, `Part(Feet)`, `uses: ["Melee", "Unarmed", "Short bow"]`.
- Tests: a kick deals natural damage with a bow (or a sabre) in hand; it trains Unarmed, not Bow; it is
  refused without a short bow in hand and without feet, each with its message; a lunge's `Wielding`
  still behaves as before.

### 4. Techniques that shoot
- `Shot { accuracy, damage_pct }` payload: `perform` fires through `ranged` with the mods; `check_target`
  uses the flight line (first creature in line, as a plain shot), within `reach`. Refusals come from
  `ready_fire` ("You have no arrows.").
- `Dash { tiles, away }`: with `away`, the actor steps back along the line from the target first, needing
  open, empty tiles behind; the shot then fires from the new tile (so a step back can clear point blank).
- Content: `PartingShot.ron` (dash away 1, then `Shot { accuracy: 2 }`, reach 6, recharge 6 turns,
  `Wielding(["Short bow"])`).
- Tests: the parting shot steps back then shoots; refused with no room behind, no arrows, or out of
  reach; the arrow lands and breaks like a plain shot; it trains Bow and Archery like a shot; the preview
  matches.

### 5. The style
- `Does::SteadyClose`: a principle that waives the point-blank penalty for whoever follows the style
  (checked in `ranged::point_blank`'s caller, so the preview shows it).
- `BowDancing.ron`: follows `["Short bow"]`; `SteadyClose` from level 2; invents the push kick at 2, the
  sweep kick at 3, the parting shot at 4. Skill "Bow dancing" in `skills.ron`; a discovery in
  `discoveries.ron` (Archery 3, Unarmed 2).
- A form: the three as a `Steps` group, "the bow dance" (push kick → sweep kick → parting shot, each
  step a little surer, more XP along the run), so `Steps` display has content beside the tri-sword's
  `Turns`.
- ADR 0024 amendment for slices 3 to 5.
- Tests: the discovery fires from the skills; following it waives point blank only from level 2;
  kicks, bashes and shots all train it; it invents at its levels; the form's bonuses along a run.

## Out of scope

- Longbows, warbows, crossbows and creatures that shoot; they come with content that wants them.
- Nocking taking longer up close, or provoking attacks.
- A kick that nocks an arrow, or other reactions beyond what principles do today.
- Balancing numbers beyond the first pass above.
