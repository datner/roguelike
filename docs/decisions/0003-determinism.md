# 0003 — Seeded determinism, enforced by clippy

Status: Superseded by 0005
Date: 2026-09-27

## Context
Worlds and bugs should be reproducible from a seed.

## Decision
All simulation randomness comes from `GameState.rng` (`rand_pcg::Pcg64`, seeded from `--seed` or `random_seed()`). `game/clippy.toml` disallows `rand::rng`, `rand::random`, clock reads, and `HashMap`/`HashSet` (random iteration order) inside `game`. `random_seed()` is the single, annotated exception.

## Consequences
- Same seed + same input sequence ⇒ same state; tests can assert exact positions.
- Changing the *order* of RNG calls changes every seeded world. That's allowed, but call it out in the commit.
- If we add save/replay, logging the input sequence alongside the seed is enough to reproduce a session.
