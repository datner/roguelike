# 0026 — Legacy: a life ends in a record of names, kept beside the saves

Status: Accepted
Date: 2026-10-30

## Context

Permadeath with legacy is a pillar (`docs/vision.md`), but a death left the last save on disk to be
continued, and nothing of a life outlived it. The next character of a lineage should find what the
last ones left (docs/plans/legacy.md), so a life needs a form that outlives its world and its content.

## Decision

- **A life ends in a `legacy::Record`**: name, origin, turn, ending, place, skill levels, styles,
  techniques (abilities left out), discoveries, gear and stash. Every entry is a name (skill name,
  blueprint name, dungeon and phase keys), never an id, so content can change under a record; a name
  that no longer exists is skipped where the record is read, never an error.
- **Every spawn from a blueprint carries `Made(name)`**, so what a thing is can be named after the
  world is gone. Saved plain; save version moved to 58.
- **An `Ending` is not a death.** `Died { by }` takes the death chain's last link (`state.death`,
  legibility.md), named when it happened, so `Command::Die` needs no cause of its own. `Abandoned` is
  a living character deleted from the menu, which ends its life like a death so ruins can't be
  rerolled. Retiring will be another variant.
- **One lineage per saves directory**: `<saves dir>/legacy.ron`, newest first, trimmed to `KEEP`
  lives, with its own version. A file of another version is never read and never overwritten until
  the next ending replaces it. The name `legacy` is no character's.
- **Burying writes the record before it removes the save**, so a failure never loses both. On death,
  `title::finish` buries instead of saving.

- **Records are an input to a new game**, like the seed and the origin
  (`GameState::new_in_lineage`). Ruins are built once, at camp, from their own stream
  (`rng::stream::LEGACY`, seeded from the world seed, never part of `Rngs`): per record the gap,
  then each left item in order. After that they are ordinary saved entities and the lineage file is
  never read again for that character.
- **How much a legacy is worth is data** (`data/legacy.ron`: `keep` and a weighted gap table, shortest
  first). An older record's gap is its newer neighbour's plus a roll, so older ruins are never better
  kept and nothing ages during play. Camp marks a ruin spot (`x`) per kept life.

- **Writings pass on knowledge, never strength.** A memoir is a lesson (`Study`, no stakes, ADR
  0017): it raises each recorded skill to at most `lesson`% of the predecessor's level and never
  lowers one, and teaches their techniques. A journal reveals their discoveries (`Revealed`), listed
  with every requirement; the inspiration itself still needs a use with stakes (ADR 0018).

## Consequences

- Death is final: there is no save to continue after it.
- A record is the only thing a new game reads from a past life: the same seed, origin and records
  give the same game.
