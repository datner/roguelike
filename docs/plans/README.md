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

Plans for tools and frontends build no game system and carry `Exempt: <reason>` instead. So does a
plan that extends another system's plan (`Exempt: extends dives; …`): its mechanisms are listed in
that plan's section, so a system is judged once, whole.

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
| `bow-dancing.md` | done | styles, techniques, combat → discovery, origins, ai, training |
| `calibration.md` | done | exempt: tooling |
| `cleanup.md` | done | exempt: refactor |
| `depth.md` | done | exempt: extends dives |
| `dives.md` | done | phases, spells → health, phases, legacy |
| `fights.md` | done | techniques, health, dives → combat, ai, lore, techniques |
| `fights-2.md` | done | exempt: extends fights |
| `floor-bot.md` | done | exempt: tooling |
| `gui.md` | done | exempt: frontend |
| `interconnection.md` | done | exempt: docs and a check |
| `legacy.md` | not started | origins, skills, techniques, discovery, dives, combat → dives, training, techniques, discovery |
| `legibility.md` | done | exempt: text only |
| `origins.md` | done | skills, styles, techniques → health, legacy |
| `phases.md` | not started | dives, skills → combat, skills, styles, discovery, dives, legacy |
| `rest.md` | done | exempt: extends health (sturdiness) |
| `skills.md` | done | combat, training, lore, health → combat, spells, techniques, styles, discovery, health, lore |
| `spells.md` | done | skills, combat → lore, ai, techniques, phases |
| `sturdiness.md` | done | skills, origins, combat, dives → skills, ai |
| `techniques.md` | done | combat, skills, styles, spells → ai, styles, training, origins, discovery |

Reads → read by. Candidates the audit found worth building, none needed to pass: an origin's starting
lore (origins → lore), fletching (crafting → bow dancing), legacy weathering by phase.
