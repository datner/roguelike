//! Every event-handler priority in the game. Lower values run first.
//! Keeping them in one place makes the relative ordering reviewable.
//! Priorities must be unique per event type (`EventRegistry::on` panics
//! otherwise); they may repeat across different events.

/// `BeforeTakeDamage`: flat armor reduction.
pub const ARMOR: i32 = 100;
