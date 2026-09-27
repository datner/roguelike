# 0002 — World mutation only through a command queue

Status: Accepted
Date: 2026-09-27

## Context
Qud-style parts react to events and change other entities. Doing that by mutating the hecs `World` from inside handlers fights the borrow checker (live query borrows) and makes bugs hard to trace.

## Decision
Input handlers and systems push `Command`s onto `GameState.commands`. `apply_commands` drains the queue and applies each one. Handlers read state, drop their borrows, then write; follow-up effects are pushed as further commands rather than applied recursively.

## Consequences
- Borrowing stays simple; every state change has one entry point to log, replay, or test.
- Ordering is explicit: a command sees the effects of every command applied before it.
- Part behaviour is expressed as event handlers that see `&World` and emit commands (see `game/src/event.rs`); the queue is what keeps them borrow-free. Handler ordering lives in `game/src/prio.rs`.
