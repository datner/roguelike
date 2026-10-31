# Plan: tests that content can't break

Summary: A balance change touches data and the bench report, never a test or a screenshot; the rules the architecture states are checked as properties over generated play.
Read first: `game/CLAUDE.md` (Tests), `front/CLAUDE.md`, `gui/CLAUDE.md`, ADR 0005 (determinism), ADR 0008 (saves), ADR 0013 (previews).

## Goal

The first pacing pass (`fa851e6`) changed a few numbers in `data/` and had to edit nine tests in two
crates and fifteen screenshots: tests asserted what the content says ("health 20/20", "Rest: 25", "the
wanderer carries a bow, a quiver and a torch") instead of what the code does. After this plan:

- Game tests check mechanisms with values of their own, or relations to the data they load ("rest is
  125% of max health"), never the shipped numbers. A check proves it by running them against a copy
  of `data/` with its numbers nudged.
- `front` and `gui` tests and the shots run on frozen content of their own, refreshed only when the
  data's format changes.
- Determinism, save round-trips, previews agreeing with actions and the world's invariants are checked
  over generated seeds and action sequences, not a few hand-picked examples.

## Decisions

- **One data path per test crate**: `game::testkit::data()`, `front`'s `tests::data()`, `gui`'s
  `testing::data()`. The game's honours `ROGUELIKE_TEST_DATA`, so a script can point every game test at
  another copy; a check fails on any other `../data` in test code.
- **Nudged content** (`scripts/nudge.sh`): copy `data/`, raise the balance numbers (health, damage,
  armor, skill levels, spawn counts), run the game tests on the copy. A test that fails there pins
  content. Part of `scripts/check.sh`: the tests are already built, so it costs one more test run.
- **Frozen content for `front`, `gui` and shots**: `fixtures/data/`, a copy of `data/`, refreshed by
  `scripts/fixtures.sh` only when the format changes (a refresh that moves a shot is then a deliberate
  act, committed with the format change). Loading real `data/` is the game's tests' business.
- **Design claims belong to the bench**: "the rat pack is at least four" is a balance target, not a
  test.
- **Properties with `proptest`** (dev-dependency of `game` only, so the no-global-RNG rule in play is
  untouched). States are generated as a seed plus a sequence of `PlayerAction`s played through the real
  turn loop, so every generated state is reachable and a failure shrinks to a short sequence. Cases are
  capped (64) in `check.sh`; `PROPTEST_CASES` raises them by hand.

## Slices

### 1. One data path, and the nudge — done
- `testkit::data()` (honouring `ROGUELIKE_TEST_DATA`) and every game test through it; `front` and
  `gui` through their own helper.
- `scripts/nudge.sh`; its output is the list of tests slice 3 fixes. Not yet in `check.sh`.
- Found: `balance::a_build_starts_from_its_origin`, `health::a_level_of_toughness_…`,
  `health::origins_start_at_their_own_health`, `origin::each_origin_starts_as_described`,
  `technique::a_kick_strikes_with_the_leg_…`. Numbers only move up, so a lower bound
  (`ai::a_rat_outpaces_you…`'s "at least four") passes it; slice 3 moves that one by hand.

### 2. Frozen content for front, gui and shots — done
- `fixtures/data/` and `scripts/fixtures.sh`; `front` tests, `gui` tests and shots load it. Taken from
  today's `data/`, so no reference moved.

### 3. Game tests check mechanisms — done
- Every test `nudge.sh` lists: values of its own (inline blueprints, `bare_state`), or a relation to the
  data it loaded. Content claims move to `bench/targets.ron` or go.
- `nudge.sh` and the `../data` check join `check.sh`.
- Done: the five, plus `origin::an_origin_says_what_it_starts_with` (a kit, not a number, so the nudge
  missed it; the leather broke it). `ai::a_rat_outpaces_you_and_comes_in_a_pack` went: both its claims
  are content's, and the dungeon tests already check spawn counts against the data.

### 4. Properties — done
- Generator: seed + `Act`s through `time::take_turn` (`game/src/properties.rs`); an `Act` picks by
  index among what the state offers (a direction, a target in range, a known spell).
- Determinism (same seed and acts, same save string); save → load → save is identity, and a loaded
  game plays on like the unsaved one; a fire's, throw's, cast's and technique's preview with no problem
  takes a turn and one with a problem is refused, a missile flies the previewed path and deals no more
  than the previewed damage; after every turn, health within its max, no creature inside a wall, no two
  on a tile.
- Proven on a planted bug (a component dropped from `with_components!`): caught and shrunk to seed 0.
- Not done: "an effect is active exactly while its timestamps say so" has no observable to compare
  with beyond what `effect.rs`'s tests do; the rest budget is unsigned, so it can't go negative.

## Out of scope

- Making the bench a test: it measures, the targets judge.
- Mutation testing of the code itself.

## Connections

Exempt: tests and checks; builds no game system.
