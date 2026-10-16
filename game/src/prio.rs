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

/// `CheckCast` and `PayCost`: the mana discipline. After cooldowns, though
/// the order only decides whose refusal is reported: a school is claimed by
/// one discipline (checked at blueprint load).
pub const MANA: i32 = 200;

/// `CheckCast`: a caster's summon limit. After the disciplines, so a
/// refused summoning still shows its price.
pub const SUMMON_LIMIT: i32 = 300;

/// `CheckCast` and `PayCost`: blood magic.
pub const BLOOD: i32 = 250;

/// `CheckCast` and `PayCost`: a scroll, which claims its own cast. First, so
/// nothing else prices a scroll.
pub const SCROLL: i32 = 10;

/// `ModifyCast`: a worn focus.
pub const FOCUS: i32 = 100;

/// `CastRisk`: the caster's chance to fizzle.
pub const FIZZLE: i32 = 100;

/// `AttackOdds`: a defender's `Footwork` adds to its perfect-dodge chance.
pub const FOOTWORK: i32 = 100;

/// `AttackOdds`, tests only: `testkit::SureHits` makes every blow its
/// holder gives or takes land, after everything else has had its say.
#[cfg(test)]
pub const SURE_HITS: i32 = 10_000;
