# 0019 — Styles

Status: Accepted
Date: 2026-10-17

## Context

Discovery (ADR 0018) can grant more than a passive proficiency: a style, a
practice the character takes up, which grows by being followed and changes
how they act once developed (`docs/vision.md`). Plan:
`docs/plans/skills.md`, slice 7.

## Decision

- **A style is a blueprint entity the creature holds.** A known style is
  carried (`Inventory`), so it is inert and saved and despawned with its
  holder like anything carried; the followed one is attached, so its parts
  take part in the holder's events. Item views skip styles
  (`view::carried`).
- **`Style { domains, skill, follows, principles }`.** One followed per
  domain; a hybrid lists several domains and sets aside whatever it
  shares one with. `Command::FollowStyle` (a turn) takes a known style up
  or sets the followed one aside.
- **A style has its own skill** (Fencing in `skills.ron`), trained inside
  `skill::train` alongside any use whose skills include everything the
  style `follows`. It never counts toward a use's level, so a fresh style
  neither helps nor hurts.
- **Principles switch on at levels of that skill.** They live in the
  `Style` component as typed `Principle { from_level, does }` entries,
  handled by `Style` handlers: one component per style, no generic property
  map. The first is `Riposte`: on `PerfectDodge`, a free `Attack` back.
- **Discoveries grant styles** (`Grant::Style`); a requirement on a
  style's level is a `Skill` requirement on its skill.
- First content: Fencing (follows Pierce, One-handed, Empty offhand;
  Riposte from 5), discovered at Melee, Pierce, One-handed and Empty
  offhand 3; the rapier, a piercing, forgiving blade.

## Consequences

- Principles that apply only while the current use follows the style, and
  penalties for not following it, need a handler to know the use's skills;
  they come with the first principle that needs them.
- A riposte is an ordinary queued attack: two fencers can trade ripostes
  until one misses; the command cap bounds the worst case.
- Inventions (techniques from styles) wait for the techniques plan.

## Amendment (ADR 0024): inventions

A style lists `invents`: techniques it teaches at levels of its skill, once
reached during a use with stakes (from `skill::train`, like discovery), never
through practice or lessons. A set-aside style keeps what it invented. The
ideas journal shows a followed style's invention once its skill is within
two levels. Fencing invents the lunge at 3; Brawling (Melee and Unarmed,
discovered at Melee, Unarmed and Dodge 3) invents the shove at 3 and
ripostes from 4.
