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
/// A step onto rubble (docs/plans/fights-2.md): it slows, both ways.
pub const RUBBLE_STEP: i64 = 2 * MOVE;
/// Hauling out after being shoved into deep water (docs/plans/fights-2.md).
pub const FLOUNDER: i64 = STANDARD;
pub const WAIT: i64 = STANDARD;
pub const PICK_UP: i64 = STANDARD;
pub const DROP: i64 = STANDARD;
pub const EQUIP: i64 = STANDARD;
pub const UNEQUIP: i64 = STANDARD;
pub const THROW: i64 = STANDARD;
pub const FIRE: i64 = STANDARD;
pub const CAST: i64 = STANDARD;
pub const PERFORM: i64 = STANDARD;
/// Taking up or setting aside a style: a change of mind, but a turn.
pub const FOLLOW_STYLE: i64 = STANDARD;
/// Studying a lesson takes a while.
pub const STUDY: i64 = 5 * STANDARD;
pub const PRACTICE: i64 = STANDARD;
/// Making one thing (docs/plans/crafting.md); a batch is a run of them.
pub const CRAFT: i64 = 2 * STANDARD;
/// Closing a door; opening one is the move into it.
pub const CLOSE: i64 = STANDARD;
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
/// rather than actions themselves, so a bump-attack costs one move. Every
/// variant is named, so a new command fails to compile until it is priced.
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
        Command::Perform { actor, .. } => Some((actor, PERFORM)),
        Command::FollowStyle { actor, .. } => Some((actor, FOLLOW_STYLE)),
        Command::Study { actor, .. } => Some((actor, STUDY)),
        Command::Practice(actor) => Some((actor, PRACTICE)),
        Command::Craft { actor, .. } => Some((actor, CRAFT)),
        Command::Close(actor) => Some((actor, CLOSE)),
        Command::Descend(actor) | Command::Ascend(actor) => Some((actor, STAIRS)),
        Command::WaitForPhase { actor, .. } => Some((actor, WAIT_FOR_PHASE)),
        Command::Nock { actor, .. } => Some((actor, NOCK)),
        Command::Quiver { actor, .. } => Some((actor, QUIVER)),
        // Free: the steps pay for sprinting.
        Command::Sprint(_) => None,
        // Consequences of an action, already paid for by it.
        Command::Ride { .. }
        | Command::Recharge { .. }
        | Command::Fall(_)
        | Command::Sink(_)
        | Command::SpendMana { .. }
        | Command::Attack { .. }
        | Command::DealDamage { .. }
        | Command::Die { .. }
        | Command::UseUp { .. }
        | Command::Dismiss(_)
        | Command::ApplyEffect { .. }
        | Command::ModifyDuration { .. }
        | Command::FindItems(_) => None,
        // The scheduler's and timers' own, done by no one.
        Command::ExpireEffect(_)
        | Command::BleedTick(_)
        | Command::Regenerate(_)
        | Command::Notice { .. }
        | Command::SetRoute { .. }
        | Command::AdvanceClock { .. }
        | Command::UpdateFov => None,
    }
}
