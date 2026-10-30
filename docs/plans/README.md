# Plans

Build plans, one per file, each a handoff: goal, decisions, slices marked done as they land, out of
scope. A game system earns its place by reaching into the others and being read back (`docs/vision.md`,
"Interconnection"); this page holds the rule that makes every plan say how.

## The rule

Every game-system plan ends with a `## Connections` section of two lists:

- **Reads**: systems this one takes as input;
- **Read by**: systems that take this one as input.

Each entry is `- <system>: <mechanism>`, the system one word from the list below. A **connection is a
mechanism, not a mention**: a function, a data field, an event or a condition through which one
system changes what another does. "Phases affect styles" is not one; "`Style.thrives` multiplies
style XP in a phase, read in `style.rs`'s training" is. A connection that only shows text (a log line,
a panel) does not count.

Each list names **at least two systems**. A connection planned but not built is marked
*(planned: <plan or slice>)* and counts; one only in the vision is marked *(vision)* and does not.

Plans for tools and frontends build no game system and carry `Exempt: <reason>` instead.

`scripts/connections.sh` checks this, run by `scripts/check.sh`. A new system joins the list below
when its first plan is written.

Systems: skills, styles, discovery, techniques, spells, combat, lore, training, origins, phases, dives, legacy, health, ai, factions, situations, talking, crafting, endings, judgement, settlers

## Template

```markdown
## Connections

**Reads**
- <system>: <the mechanism, with a code pointer once built>
- <system>: …

**Read by**
- <system>: …
- <system>: …
```

## Plans

| Plan | Status | Connects |
| --- | --- | --- |
| `agent-tooling.md` | done | exempt: tooling |
| `balance.md` | done | exempt: tooling |
| `bow-dancing.md` | done | |
| `calibration.md` | done | exempt: tooling |
| `cleanup.md` | done | exempt: refactor |
| `depth.md` | done | |
| `dives.md` | done | |
| `fights.md` | done | |
| `fights-2.md` | done | |
| `floor-bot.md` | done | exempt: tooling |
| `gui.md` | done | exempt: frontend |
| `interconnection.md` | in progress | exempt: docs and a check |
| `legacy.md` | not started | |
| `legibility.md` | done | |
| `origins.md` | done | |
| `phases.md` | not started | |
| `rest.md` | done | |
| `skills.md` | done | |
| `spells.md` | done | |
| `sturdiness.md` | done | |
| `techniques.md` | done | |
