# 0027 — Phases lean the whole world, read through one accessor

Status: Accepted
Date: 2026-10-30

## Context

A phase was a name, a line for waiting, and a filter on spawn entries. The vision wants each phase to
lean the whole world one way, for everyone, and the character systems to read it
(docs/plans/phases.md).

## Decision

- **A phase is data** (`data/phases.ron`): key, name, wait line, and optional `element`, `lean` (%)
  and `opposite`. The opposite must exist and name it back, checked at load. The Stillness is a phase
  like the others whose lists are empty, never "no phase".
- **An element is one kind of lean, not what a phase is.** Nothing may decide behaviour by matching on
  a phase's element; later phases may have none.
- **The lean is symmetric.** Damage carrying the phase's element is `lean`% stronger whoever deals it,
  applied in `BeforeTakeDamage::new` before armor: the one place damage enters, shared by the real hit
  and `preview_damage`, so a preview and the blow agree.
- **`phase::current` is the one reader of which phase holds**: a dive's own (fixed in `Place::Floor`),
  at camp the one being waited for.
- **Choosing a phase is one seam**: `phase::refusal(state, index)` says why a phase can't be waited for,
  read by `WaitForPhase`'s input check and by `phases()` (`PhaseView.refused`) for the chooser. Today
  every phase can be chosen at camp; a cycle, a ritual or earned phases replace `refusal` only.
- `DamageType::Acid`, the Mire's element; armor resists it like any kind. Save version 63.

## Consequences

- Fire spells in the Embers hit harder for both sides; the bench's Embers scenarios moved with it.
- Rules, attunement, thriving styles and phase discoveries (later slices) read `phase::current` too.
