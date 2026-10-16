//! Every event-handler priority in the game. Lower values run first.
//! Keeping them in one place makes the relative ordering reviewable.
//! Priorities must be unique per event type (`EventRegistry::on` panics
//! otherwise); they may repeat across different events.

/// `BeforeTakeDamage`: flat armor reduction.
pub const ARMOR: i32 = 100;

/// `AfterHit`: roll the attacker's (and its equipment's) on-hit effects.
pub const ON_HIT: i32 = 100;

/// `CheckCast` and `PayCost`: the cooldown discipline.
pub const COOLDOWN: i32 = 100;
