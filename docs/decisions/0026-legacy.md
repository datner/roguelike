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

## Consequences

- Death is final: there is no save to continue after it.
- A record is the only thing a new game will read from a past life (ruins, slice 3): the same seed,
  origin and records give the same game.
- `KEEP` is a constant until `data/legacy.ron` holds it.
