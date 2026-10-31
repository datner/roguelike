# Plans

Build plans, one per file, each a handoff: goal, decisions, slices marked done as they land, out of
scope. A game system earns its place by reaching into the others and being read back (`docs/vision.md`,
"Interconnection"); this page holds the rule that makes every plan say how.

## Reading them

`scripts/plans.sh` lists every plan with its status and summary, and the open slices under it.
`scripts/plans.sh NAME` outlines one (header, sections with their sizes, slices);
`scripts/plans.sh NAME PART...` prints only those parts: a section by its heading's first word
(`decisions`, `out`), a slice by its number (`3`, `1b`). Read a whole plan only to edit most of it.

## Format

```markdown
# Plan: <title>

Summary: <one sentence: what exists once it is built>
Read first: <docs, ADRs and code a builder needs, optional>.

## Goal
## Decisions
## Slices
### <n>. <title> — open        (`— done` once committed; `— done (ADR 0022)` may name what it added)
## Out of scope
## Connections                  (or an `Exempt:` line, see the rule)
```

Other `##` sections (follow-ups, as built, open questions) go after these; `###` is only for slices.
`scripts/plans.sh --lint`, run by `scripts/check.sh`, checks the title, `Summary:`, `## Goal`,
`## Slices` and every slice heading.

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

## Candidates

Connections the audit found worth building, none needed to pass: an origin's starting lore (origins →
lore), fletching (crafting → bow dancing), legacy weathering by phase.
