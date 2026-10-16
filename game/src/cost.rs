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
        _ => None,
    }
}
