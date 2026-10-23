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

## Amendment (docs/plans/bow-dancing.md): what is in hand, domains

A style may also ask what is in hand: `wielding` lists skills the wielded
weapon must have (read from the hand through `combat::blow_strike`'s default
strike, not from the use). Bow dancing follows nothing in particular
(`follows: []`) while wielding a short bow, so a shot, a bash and a kick all
train it, and the same kick with a sabre does not. A use now also follows a
style only if one of its skills is in one of the style's domains, so casting
a spell with a bow in hand trains no martial style. Existing styles behave
as before. A second principle, `SteadyClose`, waives point blank (ADR 0013);
`style::principle` answers whether a followed style has one switched on.

A third principle, `Gather` (bow dancing from level 1): with every action
its follower takes, arrows lying on or next to its tile go back in its
quiver, free (`ammo::gather`, run where an action's energy is charged).
