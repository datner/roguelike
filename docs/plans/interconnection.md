# Plan: interconnection audit

Handoff document. Read `docs/vision.md` ("Interconnection" first, then the whole of it), every plan in
`docs/plans/`, and `docs/decisions/README.md` first.

## Goal

The vision's thesis is a game as interconnected as possible: a system earns its place by reaching into
the others, and the others read it back. Today that is a sentence in the vision; nothing checks it, and
plans list what they build, not what they connect to. After this plan every game-system plan says, in
one fixed section, which systems it reads and which read it, each through a named mechanism. A plan
with fewer than two connections in either direction gets slices that add them. A check keeps new plans
honest.

## Decisions taken for this plan

- **A connection is a mechanism, not a mention**: a function, a data field, an event or a condition
  through which one system changes what another does. "Phases affect styles" is not one; "`Style.thrives`
  multiplies style XP in a phase, read in `style.rs`'s training" is. A connection that only shows text
  (a log line, a panel) does not count.
- **Every game-system plan has a `## Connections` section** with two lists:
  - **Reads**: systems this one takes as input, each with its mechanism;
  - **Read by**: systems that take this one as input, each with its mechanism.
  Each list has **at least two other systems**. A connection planned but not built is marked
  *(planned: <plan or slice>)*; one only in the vision is marked *(vision)* and does not count toward
  the two.
- **The systems** a connection may name (one word each, so the check can read them): skills, styles,
  discovery, techniques, spells, combat, lore, training, origins, phases, dives (camp, dungeons and
  floors), legacy, factions, situations, talking, crafting, endings, judgement, settlers. New systems
  join this list in `docs/plans/README.md` when their first plan is written.
- **Exempt** plans: tools and frontends, which build no game system: `gui.md`, `agent-tooling.md`,
  `balance.md`. They carry `Exempt: <reason>` in place of the section.
- **Built plans are not rewritten.** Their history stays; the `## Connections` section is added at the
  end, describing what exists with code pointers, and any missing connection becomes a slice of a new
  plan (below), never a silent edit to a finished one.
- **Enforced by `scripts/check.sh`**: every `docs/plans/*.md` other than the README has either a
  `## Connections` section whose **Reads** and **Read by** lists each name at least two systems from the
  README's list (counting only entries not marked *(vision)*), or an `Exempt:` line. Quiet on success,
  names the plan and what it lacks on failure, like the other steps.
- **The map**: `docs/plans/README.md` holds the systems list, the rule above, the plan template's
  `## Connections` stub, and one line per plan (status and the systems it connects). The vision's
  "Interconnection" section points at it. `CLAUDE.md`'s pointer to `docs/plans/` mentions the rule.

## Slices

Each slice ends with `scripts/check.sh` green and a commit, and is marked done here first.

### 1. The rule and the check — done
- As built: `scripts/connections.sh` (run by `check.sh`, `--test` runs it on
  `scripts/fixtures/connections/`). The systems list gains `health` (rest, sturdiness) and `ai`,
  which built plans connect through. A section whose body is `*TODO*` passes until slice 4. `ripgrep`
  joins the devShell for the script.
- `docs/plans/README.md`: the systems list, the definition of a connection, the template stub, the
  exempt list, one line per plan.
- `scripts/check.sh`: the connections step. Until slices 2–3 land it would fail, so this slice also
  adds `Exempt:` lines to the three tool plans and a provisional `## Connections` (marked *TODO*) that
  the check reports as a warning, not a failure, for the rest; slice 4 turns warnings into failures.
- `CLAUDE.md` and the vision's "Interconnection" section point at the README.
- Tests: the check passes on a fixture plan with two and two, fails on one with one, accepts `Exempt:`,
  and ignores *(vision)* entries (a small shell test beside the other script tests, or a fixture run
  in `check.sh` itself).

### 2. Unbuilt plans: legacy and phases — done
- As built: both meet the bar from what they already plan; no gap slices were needed. Phases' "read by
  light, stamina and movement rules" are rules on every creature, not other systems, so they are not
  listed.
- Write their `## Connections` sections from what they already plan. First pass, to verify:
  - **phases** reads combat (the lean in damage), skills (attunement in `skill::train`), styles
    (`Style.thrives`), discovery (`Requirement::Phase`), dives (`Place::Floor` keeps the phase); read by
    lore? *no*, legacy (the record's `place`), balance (scenario phase; exempt, does not count), combat,
    light, stamina and movement rules. Strong already.
  - **legacy** reads origins (`Origin`), skills (levels), techniques (non-abilities), discovery
    (`Discovered`), dives (camp store, place); read by training (memoir through `Study`), discovery
    (`Revealed` in `ideas`), dives (ruins in camp). Meets the bar; phases is a candidate (the gap
    weathering differs by the phase of death) but not required.
- Any gap found becomes a slice in that plan, before it is built.

### 3. Built plans: skills, spells, techniques, bow-dancing, dives, origins — done
- As built: every built plan meets the bar, so `docs/plans/connections.md` was not needed; the
  candidates are listed under the README's table. Origins passes through health (each origin's base
  health, from sturdiness.md, written after this plan), bow dancing through origins, ai and training
  (`Trip`, the spirit wolf, `KickingPrimer`). Plans written after this one are covered too: fights,
  sturdiness get sections; depth, fights-2 and rest extend another plan and say where they are listed
  (a new `Exempt` reason in the README); legibility is text only.
- Add `## Connections` to each, describing what exists, with code pointers. First pass, to verify
  against the code:
  - **skills** reads combat (stakes), lore, training, origins; read by combat, spells, techniques,
    styles, discovery. Meets the bar.
  - **spells** reads skills (`skill::cast`), combat (damage), lore (Arcana first casts); read by lore,
    styles (magic styles invent, ADR 0016), AI. Candidates to add: phases (planned: the lean), crafting
    (vision: reagents, scrolls by scribing).
  - **techniques** reads combat (`combat::blow`), skills, styles (invented, step-of-form); read by
    training (manuals), origins (abilities), styles. Meets the bar.
  - **bow-dancing** (a style and its techniques) reads styles, techniques, combat (point-blank odds);
    read by discovery (the bow dance), balance (exempt). **Read by** is short: candidates are phases
    (the Tempest's ranged falloff, planned) and crafting (fletching makes the arrows it spends; this
    also answers the open question of breaking arrows).
  - **dives** reads phases, dungeons, camp; read by legacy (planned), phases, lore (sightings per
    floor). Candidates: situations (vision), and the cost of returning (vision open question).
  - **origins** reads skills, styles, techniques; read by legacy (planned), balance (exempt). **Read
    by** is short: candidates are factions (born into; vision), heirs (vision), and lore (an origin's
    knowledge: a goblin-born origin starts with goblin lore). One must be built to pass.
- Each missing connection becomes a slice of **`docs/plans/connections.md`**, one slice per connection,
  each small: the mechanism, its one reader, tests. Ordered by how many systems it touches.

### 4. Strict — done
- Turn the check's warnings into failures; remove every *TODO* section.
- The README's per-plan lines are complete.

## Out of scope

- Building the connection slices themselves: `docs/plans/connections.md` is written here and built
  after.
- Connections for systems that exist only in the vision (factions, crafting, endings, judgement,
  settlers): their plans carry the section from the start, under the check.
- Measuring connectedness beyond the count (a graph, a score).

Exempt: docs and a check; builds no game system.
