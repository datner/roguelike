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

/// How a blow harms. Armor resists each kind separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DamageType {
    Slashing,
    Blunt,
    Piercing,
}

impl DamageType {
    pub const ALL: [DamageType; 3] = [Self::Slashing, Self::Blunt, Self::Piercing];

    pub fn noun(self) -> &'static str {
        match self {
            Self::Slashing => "slashing",
            Self::Blunt => "blunt",
            Self::Piercing => "piercing",
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// The kinds of damage one blow carries: a set, so a thrown knife can be
/// both slashing and piercing. Serialized as a list (`[Slashing, Piercing]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "Vec<DamageType>", into = "Vec<DamageType>")]
pub struct DamageTypes(u8);

impl DamageTypes {
    pub const fn single(kind: DamageType) -> Self {
        Self(kind.bit())
    }

    pub fn of(types: &[DamageType]) -> Self {
        types.iter().copied().collect()
    }

    pub fn contains(self, kind: DamageType) -> bool {
        self.0 & kind.bit() != 0
    }

    pub fn iter(self) -> impl Iterator<Item = DamageType> {
        DamageType::ALL
            .into_iter()
            .filter(move |&kind| self.contains(kind))
    }
}

impl FromIterator<DamageType> for DamageTypes {
    fn from_iter<I: IntoIterator<Item = DamageType>>(iter: I) -> Self {
        Self(iter.into_iter().fold(0, |bits, kind| bits | kind.bit()))
    }
}

impl From<Vec<DamageType>> for DamageTypes {
    fn from(types: Vec<DamageType>) -> Self {
        types.into_iter().collect()
    }
}

impl From<DamageTypes> for Vec<DamageType> {
    fn from(types: DamageTypes) -> Self {
        types.iter().collect()
    }
}

/// Flat damage reduction per kind of damage; worn pieces add up per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Armor {
    pub slashing: i32,
    pub blunt: i32,
    pub piercing: i32,
}

impl Armor {
    pub fn uniform(value: i32) -> Self {
        Self {
            slashing: value,
            blunt: value,
            piercing: value,
        }
    }

    pub fn of(self, kind: DamageType) -> i32 {
        match kind {
            DamageType::Slashing => self.slashing,
            DamageType::Blunt => self.blunt,
            DamageType::Piercing => self.piercing,
        }
    }

    /// What this armor takes off a blow of `types`: the kind it resists
    /// *least*, so a blow carrying several kinds goes through the weakest
    /// spot (ADR 0012). A blow of no kind (effect damage) is not resisted.
    pub fn against(self, types: DamageTypes) -> i32 {
        types.iter().map(|kind| self.of(kind)).min().unwrap_or(0)
    }

    pub fn plus(self, other: Armor) -> Armor {
        Armor {
            slashing: self.slashing + other.slashing,
            blunt: self.blunt + other.blunt,
            piercing: self.piercing + other.piercing,
        }
    }
}

/// Inclusive natural (unarmed) melee damage range, boosted by Strength.
/// An equipped `Weapon` replaces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Melee {
    pub min: i32,
    pub max: i32,
}

/// A creature attribute an item can draw on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stat {
    Strength,
    Agility,
}

/// A creature's attribute scores. 16 is average: see `Stats::modifier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    pub strength: i32,
    pub agility: i32,
}

impl Stats {
    pub fn score(self, stat: Stat) -> i32 {
        match stat {
            Stat::Strength => self.strength,
            Stat::Agility => self.agility,
        }
    }

    /// Qud's bonus: +1 per 2 points above 16, -1 per 2 below (rounded down).
    pub fn modifier(self, stat: Stat) -> i32 {
        (self.score(stat) - 16).div_euclid(2)
    }
}

/// Item part: it can be thrown up to `range` tiles, hitting the first
/// creature on the way for `min..=max` plus the thrower's `stat` modifier.
/// Separate from `Weapon`, so a blade is a weaker missile than a dagger
/// is in the hand (ADR 0012: versatility is paid for in the dice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Throwable {
    pub range: i32,
    pub min: i32,
    pub max: i32,
    pub stat: Stat,
    pub types: DamageTypes,
}

/// What a launcher shoots and a quiver holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmmoKind {
    Arrow,
}

/// Item part: a loose piece of ammunition. Anything with this can be nocked
/// or put in a quiver of its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ammo(pub AmmoKind);

/// Item part on ammunition: what it does when it hits. `break_bp` is the
/// chance (out of 10 000) that it is lost when it hits a creature; one that
/// misses always lands intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Projectile {
    pub min: i32,
    pub max: i32,
    pub types: DamageTypes,
    pub break_bp: u32,
}

/// Item part: fires `ammo` up to `range` tiles when equipped. The shot
/// rolls the ammunition's dice plus the shooter's `stat` modifier. A nocked
/// arrow is the one thing in the launcher's own `Inventory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launcher {
    pub range: i32,
    pub ammo: AmmoKind,
    pub stat: Stat,
}

/// Item part: while equipped, a quiver supplies a launcher's shots. Its
/// arrows are the entities in its own `Inventory`, so they stay inert, are
/// saved and despawn with it. Loose ammunition does not stack; this is the
/// only place it does (ADR 0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quiver {
    pub ammo: AmmoKind,
    pub capacity: u32,
}

/// Item part: when equipped, melee hits roll this instead of the wielder's
/// natural `Melee`, plus the wielder's `stat` modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Weapon {
    pub min: i32,
    pub max: i32,
    pub stat: Stat,
    pub types: DamageTypes,
}

/// Entities carried by this one (equipment). Their components take part in
/// event dispatch to the owner, and they are despawned along with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Attachments(pub Vec<Entity>);

/// What a body part is, which decides what can be equipped on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PartKind {
    Head,
    Body,
    Back,
    Hand,
    Feet,
}

impl PartKind {
    /// For messages: "You have no hand to equip the knife on."
    pub fn noun(self) -> &'static str {
        match self {
            PartKind::Head => "head",
            PartKind::Body => "body",
            PartKind::Back => "back",
            PartKind::Hand => "hand",
            PartKind::Feet => "feet",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodyPart {
    pub kind: PartKind,
    /// "left hand"; several parts may share a kind but not a name.
    pub name: String,
}

/// A creature's anatomy (Qud-style): each part holds at most one equipped
/// item. Parts are referred to by index, so the list never changes order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Body(pub Vec<BodyPart>);

/// Item part: it can be equipped on a body part of this kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equippable {
    pub part: PartKind,
}

/// On an equipped item: the index of the owner's `Body` part it occupies.
/// The item is also in the owner's `Attachments`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EquippedOn(pub u32);

/// Items carried but not in use. Kept apart from `Attachments` so their
/// parts stay inert: a knife in the pack never makes anyone bleed. Despawned
/// with the carrier.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct Inventory(pub Vec<Entity>);

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

/// Marker component: a thing that can lie on the floor. Its blueprints are
/// the pool new zones scatter (`GameState::new`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item;

/// Marker component: the player has stood on this floor item, so
/// auto-explore stops heading for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found;

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
