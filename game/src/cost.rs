//! Every action's energy cost, in one place so relative costs are
//! reviewable. See ADR 0006 for the energy model.

use hecs::Entity;

use crate::command::Command;

/// `Speed` of a normal actor: it gains this much energy per tick.
pub const NORMAL_SPEED: u32 = 100;

/// Ticks between a normal actor's standard actions; also one displayed turn.
pub const TICKS_PER_TURN: u64 = 100;

/// A standard action: a normal-speed actor affords one every
/// `TICKS_PER_TURN` ticks.
pub const STANDARD: i64 = NORMAL_SPEED as i64 * TICKS_PER_TURN as i64;

pub const MOVE: i64 = STANDARD;
pub const WAIT: i64 = STANDARD;
pub const PICK_UP: i64 = STANDARD;
pub const DROP: i64 = STANDARD;
pub const EQUIP: i64 = STANDARD;
pub const UNEQUIP: i64 = STANDARD;
pub const THROW: i64 = STANDARD;
pub const FIRE: i64 = STANDARD;
pub const CAST: i64 = STANDARD;
/// Taking up or setting aside a style: a change of mind, but a turn.
pub const FOLLOW_STYLE: i64 = STANDARD;
/// Studying a lesson takes a while.
pub const STUDY: i64 = 5 * STANDARD;
pub const PRACTICE: i64 = STANDARD;
/// Taking the stairs, either way: a step.
pub const STAIRS: i64 = MOVE;
/// Waiting for a phase: one action, however long the wait (time is neutral).
pub const WAIT_FOR_PHASE: i64 = STANDARD;
/// Fetching a special arrow from the pack: the price of overriding the
/// quiver (ADR 0013).
pub const NOCK: i64 = STANDARD;
pub const QUIVER: i64 = STANDARD;

/// The actor that pays for `command`, and how much. `None` for commands that
/// are consequences of an action (`Attack` from a bump, damage, death…)
/// rather than actions themselves, so a bump-attack costs one move.
pub(crate) fn of(command: &Command) -> Option<(Entity, i64)> {
    match *command {
        Command::TryMove(actor, _) => Some((actor, MOVE)),
        Command::Wait(actor) => Some((actor, WAIT)),
        Command::PickUp(actor) => Some((actor, PICK_UP)),
        Command::Drop { actor, .. } => Some((actor, DROP)),
        Command::Equip { actor, .. } => Some((actor, EQUIP)),
        Command::Unequip { actor, .. } => Some((actor, UNEQUIP)),
        Command::Throw { actor, .. } => Some((actor, THROW)),
        Command::Fire { actor, .. } => Some((actor, FIRE)),
        Command::Cast { actor, .. } => Some((actor, CAST)),
        Command::FollowStyle { actor, .. } => Some((actor, FOLLOW_STYLE)),
        Command::Study { actor, .. } => Some((actor, STUDY)),
        Command::Practice(actor) => Some((actor, PRACTICE)),
        Command::Descend(actor) | Command::Ascend(actor) => Some((actor, STAIRS)),
        Command::WaitForPhase { actor, .. } => Some((actor, WAIT_FOR_PHASE)),
        Command::Nock { actor, .. } => Some((actor, NOCK)),
        Command::Quiver { actor, .. } => Some((actor, QUIVER)),
        _ => None,
    }
}
