# Plan: bow dancing

Handoff document. Read `docs/plans/techniques.md`, ADR 0013 (ranged), ADR 0017 (skills), ADR 0019
(styles), ADR 0024 (techniques) and `game/CLAUDE.md` first.

## Goal

A martial style for fighting up close with a short bow in hand: the legs do the close work (kicks that
push and trip) and the bow finishes at arm's length. Kicks are not the style's: anyone with feet can
kick, for any reason. With a bow in hand they are simply the only good way to fight in melee, short of
dropping the bow or risking it as a club; the style is what makes them flow with the bow. Today the bow
has no weakness up close, so such a style would fix nothing; first, shooting next to an enemy gets
harder for everyone, then the style answers that. A longbow, warbow or crossbow is too big to dance
with; only a short bow qualifies.

## Decisions taken for this plan

- **Point blank is hard for everyone.** Firing a launcher while an enemy stands next to the shooter
  costs `POINT_BLANK` accuracy (start at 4, the size of a few skill levels). It is worked out in
  `ranged::ready_fire`, which the command, `aim` and AI share, so the preview shows the lower chance.
  Thrown things are not affected (a knife thrown up close is fine). No creature shoots yet, so in play
  this only touches the player today; it is a rule, not a player special case.
- **"Short bow" is a skill tag.** A new skill with `weight: 0`: it never counts toward a use's level
  (`skill::level` already skips weightless skills in its average) but is among a weapon's skills, so
  `Need::Wielding(["Short bow"])` and a style can name it. The short bow `Trains(["Bow", "Short
  bow"])`; a longbow later trains only `"Bow"`. A weightless skill gains XP like any other; it is not
  shown in the skills panel.
- **A kick is a general technique.** `Blow { unarmed: true }` strikes with the creature's natural
  `Melee`, whatever it wields, and trains what an unarmed blow trains. It needs feet, nothing in hand:
  a new `Need::Part(PartKind)` ("Your push kick needs feet."). A brawler, a swordsman or an archer can
  all learn one.
- **A style can follow what is in hand.** `Style.wielding: Vec<skill>`: a use follows the style when it
  uses all of `follows` *and* the wielded weapon has all of `wielding` (`skill::melee`'s weapon skills,
  read from the hand, not from the blow). Bow dancing follows `[]` while wielding `["Short bow"]`, so a
  kick, a shot and a bash with a short bow in hand all train it, and the same kick with a sabre in
  hand does not. Existing styles have `wielding: []` and behave as before.
- **A technique can shoot.** A `Shot { accuracy, damage_pct }` payload fires the next arrow through
  `ranged::ready_fire` and the same launch a plain shot uses (flight, odds, breaking, training), with
  the bonuses added. Its reach is the technique's `reach`. `Dash` gains `away: bool` to step back from
  the target before acting.
- **The style shapes kicks, it does not own them.** Through `StyleBonus` on the kicks and the parting
  shot (surer, harder, cheaper recharge while following it), and by inventing them for a follower who
  does not know them yet. Discovered from Archery 3 and Unarmed 2. Named "Bow dancing" (skill and
  style; renaming it is a data change).
- Amendments, not new ADRs: ADR 0013 (point blank), ADR 0019 (`wielding`), ADR 0024 (unarmed blows,
  `Part`, `Shot`, `Dash` away).

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first. Every changed
component: blueprint `ComponentDef`, `SavedComponent` + round-trip test, `SAVE_VERSION` bump.

### 1. Point blank — done
- `ranged::point_blank(state, actor) -> bool`: an enemy (`side::enemies`) on any of the 8 tiles around
  the shooter. `ready_fire` lowers `strike.accuracy` by `POINT_BLANK` when it holds.
- `front/targeting.rs`: the aim status says "point blank" when the penalty applies, from a flag on
  `aim`'s result (never worked out in `front`).
- ADR 0013 amendment.
- Tests: the hit chance drops with an adjacent enemy and not with an adjacent ally or a distant enemy;
  the preview and the shot agree; throwing is unaffected; seeded shots draw the combat stream as before.

### 2. The short bow tag — done
- `data/skills.ron`: `(name: "Short bow", domain: Martial, weight: 0)`. `ShortBow.ron` trains both.
- The skills panel (`view`/`front::text`) leaves out weightless skills.
- Tests: the tag changes no level (a shot's hit chance is the same with and without it); a short-bow
  shot and bash train it; it is not listed in the panel.

### 3. Kicks
- `Blow { accuracy, damage_pct, unarmed }`: `BlowMods.unarmed` makes `combat::blow_strike` skip the
  wielded weapon for the natural `Melee`. `aim` previews it through the same function.
- `Need::Part(PartKind)` checks the `Body`.
- Content: `PushKick.ron` (reach 1, unarmed, `Part(Feet)`, `damage_pct: -25`, `Shove { tiles: 1 }`
  rider, recharge 3 turns). The trip stays as it is (no needs, so the spirit wolf, which has no `Body`,
  still trips); it is the second kick of the form in slice 5. A manual teaching the push kick joins
  the finds, so it is not only the style's.
- Tests: a kick deals natural damage with a bow, a sabre or nothing in hand; it trains Unarmed, not
  the weapon's skills; it is refused without feet, with its message.

### 4. Techniques that shoot
- `Shot { accuracy, damage_pct }` payload: `perform` fires through `ranged` with the mods; `check_target`
  uses the flight line (first creature in line, as a plain shot), within `reach`. Refusals come from
  `ready_fire` ("You have no arrows.").
- `Dash { tiles, away }`: with `away`, the actor steps back along the line from the target first, needing
  open, empty tiles behind; the shot then fires from the new tile (so a step back can clear point blank).
- Content: `PartingShot.ron` (dash away 1, then `Shot { accuracy: 2 }`, reach 6, recharge 6 turns; no
  needs beyond a launcher and an arrow, which `ready_fire` already asks for).
- Tests: the parting shot steps back then shoots; refused with no room behind, no arrows, or out of
  reach; the arrow lands and breaks like a plain shot; it trains Bow and Archery like a shot; the preview
  matches.

### 5. The style
- `Style.wielding` (see decisions), checked where a use's following is decided in `style.rs`.
- `Does::SteadyClose`: a principle that waives the point-blank penalty for whoever follows the style
  (checked in `ranged::point_blank`'s caller, so the preview shows it).
- `BowDancing.ron`: `follows: []`, `wielding: ["Short bow"]`; `SteadyClose` from level 2; invents the
  push kick at 2 and the parting shot at 4 (skipped when already known, as discoveries are). Skill
  "Bow dancing" in `skills.ron`; a discovery in `discoveries.ron` (Archery 3, Unarmed 2).
- Bonuses: the push kick and the trip gain accuracy and recharge faster while following bow dancing;
  the parting shot gains damage from level 3.
- A form: push kick → trip → parting shot as a `Steps` group, "the bow dance" (each step a little
  surer, more XP along the run), so `Steps` display has content beside the tri-sword's `Turns`. The
  steps keep working on their own, outside the form.
- ADR 0019 and 0024 amendments.
- Tests: the discovery fires from the skills; a kick follows the style with a short bow in hand and
  not with a sabre; existing styles follow as before; following it waives point blank only from level
  2; it invents at its levels and skips what is known; the form's bonuses along a run.

## Out of scope

- Longbows, warbows, crossbows and creatures that shoot; they come with content that wants them.
  Their close-quarters answer (a dagger in the other hand, "desperate measures") is a different idea,
  not this style.
- A bash with a bow risking the bow (the owner's remark: it can be done if you don't mind breaking
  it). Today it is only weak.
- Nocking taking longer up close, or provoking attacks.
- A kick that nocks an arrow, or other reactions beyond what principles do today.
- Balancing numbers beyond the first pass above.
