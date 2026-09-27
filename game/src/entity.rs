//! Components: plain data stored in the hecs world. Behaviour lives in
//! event handlers (see `event`) and in the command applier, never here.

use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::blueprint::BlueprintId;
use crate::cell::Rgb;
use crate::time::Tick;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Appearance {
    pub glyph: char,
    pub fg: Rgb,
    pub bg: Rgb,
}

/// Marker component: an entity with this blocks movement onto its tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlocksMovement;

/// Marker component: the player character, addressed as "you" in messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player;

/// Marker component: died but kept in the world (only the player, so the
/// frontend can still render and report on it). The scheduler skips it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dead;

/// Bare noun without an article, e.g. "goblin"; `grammar::Subject` adds "the".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Name(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub current: i32,
    pub max: i32,
}

/// Armor value: flat reduction applied to each incoming hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Armor {
    pub av: i32,
}

/// Inclusive melee damage range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Melee {
    pub min: i32,
    pub max: i32,
}

/// Entities carried by this one (equipment). Their components take part in
/// event dispatch to the owner, and they are despawned along with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Attachments(pub Vec<Entity>);

/// Energy gained per tick; `cost::NORMAL_SPEED` is normal. See `time`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Speed(pub u32);

/// Scheduler energy. The actor may act while this is `>= 0`; acting
/// subtracts the action's cost (`cost`). Runtime state, not blueprint data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Energy(pub i64);

/// Spawn order, assigned by `GameState::spawn`. Breaks scheduling ties so
/// turn order never depends on hecs `Entity` ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SpawnSeq(pub u64);

/// Marker component: acts against the player (see `ai`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hostile;

/// Blueprint-side length of a timed effect, in ticks. `ApplyEffect` swaps it
/// for `Timed`, since only then is the start known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Duration(pub u64);

/// A timed effect's lifetime, stored relative to its start so a changed
/// duration needs no bookkeeping. The end is always derived, and whether the
/// effect is active is answered from these timestamps alone, never from
/// timer state (ADR 0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timed {
    pub start: Tick,
    pub duration: u64,
}

impl Timed {
    pub fn end(self) -> Tick {
        self.start.after(self.duration)
    }

    pub fn is_active(self, now: Tick) -> bool {
        now < self.end()
    }
}

/// The entity a timed effect is attached to (it is also listed in the
/// target's `Attachments`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EffectTarget(pub Entity);

/// Effect part: `damage` to the target every `interval` ticks while active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bleeding {
    pub damage: i32,
    pub interval: u64,
}

/// Effects applied to the victim when the holder, or the creature it is
/// attached to, lands a melee hit that deals damage. Built from
/// `ComponentDef::OnHit` at load; not serializable, since it holds ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnHit(pub Vec<OnHitEntry>);

/// Each entry rolls on its own; every success applies a new effect instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnHitEntry {
    pub effect: BlueprintId,
    /// Out of `effect::BASIS_POINTS`.
    pub chance_bp: u32,
}
