# 0015 — Sides and summons

Status: Accepted
Date: 2026-10-16

## Context

Summoned creatures fight for their summoner, so the AI can no longer assume
that every hostile wants the player and nothing else acts. Area spells also
need to know who is a friend (`Hits::Enemies`). Perception was built around
one viewer: the player's field of view, reused by every hostile (ADR 0007).

## Decision

- **Two sides, derived, not stored.** `side::side` is `Hostile` for a
  creature with `Hostile`, `Player` otherwise, and a summon's is its
  owner's (`Summoned { owner }`, followed a few owners deep). Factions can
  replace the derivation later without touching its callers.
- **AI goes for the nearest enemy it perceives** (ties by `SpawnSeq`).
  Hostiles and summons act; anything else waits. A summon with nothing to
  fight follows its owner. Walking into your own summon trades places.
- **Perception stays one view.** The player is perceived through the
  player's view, as before. Anyone else is perceived only when both
  creatures are in the player's view, within sight radius, with open
  ground on the line between. No view is computed per actor, so creatures
  the player can't see don't fight each other: off-screen battles never
  happen. That's a deliberate simplification, not a sight model.
- **A summon is a spawn plus a timed effect.** The `Summon { blueprint,
  count, lifetime }` spell part spawns through `GameState::spawn` with
  `Summoned`, then attaches a `Binding` effect (`Binding.ron`) whose
  `Timed` is the lifetime. Expiry goes through `effect::expire_effect`,
  which dismisses the target (`Command::Dismiss`); `ModifyDuration`
  lengthens or shortens it like any effect (ADR 0006). An owner's death
  dismisses its summons.
- **Placement is seeded.** Summons appear on the free open tiles nearest
  the aimed spot, in view of it and within 2 tiles. Equally near tiles are
  shuffled from a new `spell` RNG stream (ADR 0005), ring by ring and only
  as far as needed. The aim preview places them on a clone of the stream,
  so it shows the tiles the cast will use.
- **A limit is a veto, not a discipline.** `SummonLimit { max }` on the
  caster refuses, in `CheckCast`, a summoning that would take it over.

## Consequences

- `SAVE_VERSION` 14: `Summoned` (owner remapped), `Summon` (blueprint by
  name), `Binding`, `SummonLimit`, and the `spell` stream in `Rngs`.
- Cast summons draw from the spell stream; nothing else does yet, so no
  existing seeded result changes.
- Revisit perception when lighting, differing sight radii or fights out of
  view matter: each breaks the shared-view assumption.
