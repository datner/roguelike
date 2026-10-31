//! Components: plain data stored in the hecs world. Behaviour lives in
//! event handlers (see `event`) and in the command applier, never here.

use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::blueprint::BlueprintId;
use crate::cell::Rgb;
use crate::skill::SkillId;
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
    /// A sprite from `data/tiles.ron`, for frontends that draw them
    /// (ADR 0023). By name, so saves and blueprints share it.
    #[serde(default)]
    pub sprite: Option<String>,
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

/// How a blow or a spell harms. Armor resists each kind separately; the
/// last three are elements, which spells carry (ADR 0014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DamageType {
    Slashing,
    Blunt,
    Piercing,
    Fire,
    Cold,
    Shock,
    /// The Mire's element (docs/plans/phases.md): it corrodes.
    Acid,
}

impl DamageType {
    pub const ALL: [DamageType; 7] = [
        Self::Slashing,
        Self::Blunt,
        Self::Piercing,
        Self::Fire,
        Self::Cold,
        Self::Shock,
        Self::Acid,
    ];

    pub fn noun(self) -> &'static str {
        match self {
            Self::Slashing => "slashing",
            Self::Blunt => "blunt",
            Self::Piercing => "piercing",
            Self::Fire => "fire",
            Self::Cold => "cold",
            Self::Shock => "shock",
            Self::Acid => "acid",
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

    /// For reading: "slashing+piercing"; empty for none.
    pub fn words(self) -> String {
        self.iter()
            .map(DamageType::noun)
            .collect::<Vec<_>>()
            .join("+")
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
/// The elemental fields are resistances, read the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Armor {
    pub slashing: i32,
    pub blunt: i32,
    pub piercing: i32,
    pub fire: i32,
    pub cold: i32,
    pub shock: i32,
    pub acid: i32,
}

impl Armor {
    /// `value` against every physical kind; no elemental resistance.
    pub fn uniform(value: i32) -> Self {
        Self {
            slashing: value,
            blunt: value,
            piercing: value,
            ..Self::default()
        }
    }

    /// How much of it there is to wear: the physical kinds summed, since
    /// resisting an element weighs nothing.
    pub fn weight(self) -> i32 {
        self.slashing + self.blunt + self.piercing
    }

    pub fn of(self, kind: DamageType) -> i32 {
        match kind {
            DamageType::Slashing => self.slashing,
            DamageType::Blunt => self.blunt,
            DamageType::Piercing => self.piercing,
            DamageType::Fire => self.fire,
            DamageType::Cold => self.cold,
            DamageType::Shock => self.shock,
            DamageType::Acid => self.acid,
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
            fire: self.fire + other.fire,
            cold: self.cold + other.cold,
            shock: self.shock + other.shock,
            acid: self.acid + other.acid,
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

/// What a creature's own blows (`Melee`) carry, beside it: a rime wisp's
/// touch is cold. Without one, a natural blow is blunt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Natural(pub DamageTypes);

/// A creature attribute an item can draw on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stat {
    Strength,
    Agility,
    Intelligence,
    Willpower,
}

impl Stat {
    /// As the character sheet names it: "STR".
    pub fn abbr(self) -> &'static str {
        match self {
            Stat::Strength => "STR",
            Stat::Agility => "AGI",
            Stat::Intelligence => "INT",
            Stat::Willpower => "WIL",
        }
    }
}

/// A creature's attribute scores. 16 is average: see `Stats::modifier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    pub strength: i32,
    pub agility: i32,
    pub intelligence: i32,
    pub willpower: i32,
}

impl Stats {
    /// Every score 16: no modifier anywhere.
    pub const AVERAGE: Self = Self {
        strength: 16,
        agility: 16,
        intelligence: 16,
        willpower: 16,
    };

    pub fn score(self, stat: Stat) -> i32 {
        match stat {
            Stat::Strength => self.strength,
            Stat::Agility => self.agility,
            Stat::Intelligence => self.intelligence,
            Stat::Willpower => self.willpower,
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

/// Which kind of magic a spell is. A caster's discipline for the school
/// decides what casting it costs (ADR 0014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum School {
    Pyromancy,
    Arcane,
    Hemomancy,
}

/// Spell part: a spell a creature knows, held in its `Attachments`. `power`
/// is an abstract magnitude; each discipline converts it into its own cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spell {
    pub school: School,
    pub range: i32,
    pub power: i32,
}

/// Spell part: hits the first creature on the line for `min..=max` plus the
/// caster's modifier for the stat of the discipline casting it, resisted by
/// armor of its `types`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellDamage {
    pub min: i32,
    pub max: i32,
    /// The kinds it is (elements, usually); none for raw force, which no
    /// armor resists.
    #[serde(default)]
    pub types: DamageTypes,
}

/// The ground a spell covers; see `area::tiles`. Distances are rings, so a
/// radius-1 burst is the 3×3 block around its centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    /// Flies like a bolt, then bursts where it lands: every tile within
    /// `radius` of that spot that the spot can see.
    Burst { radius: i32 },
    /// A quarter circle from the caster toward the aimed tile, out to the
    /// spell's range.
    Cone,
    /// A straight line from the caster toward the aimed tile, through
    /// creatures, until a wall or the spell's range.
    Beam,
    /// Every tile within `radius` of the caster that the caster can see; not
    /// aimed.
    Nova { radius: i32 },
}

/// Who in an area a spell affects. Sides are hostile or not, until there are
/// factions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hits {
    /// Everyone, the caster included.
    All,
    /// Creatures on the other side from the caster.
    Enemies,
    /// The caster and creatures on its side.
    Allies,
}

/// Spell part: the spell covers an area instead of hitting the first
/// creature on a line. Each creature caught takes its own damage roll, less
/// `falloff` per tile from the area's centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Area {
    pub shape: Shape,
    pub hits: Hits,
    #[serde(default)]
    pub falloff: i32,
}

/// AI part: the creature casts the spells it knows when one would hit its
/// target and spare its own side, before it fights hand to hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellAi;

/// AI part: the creature performs the techniques it knows when one would
/// strike its target and spare its own side, before it bumps or steps
/// (ADR 0024).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechniqueAi;

/// Spell part: the spell does not go off when cast but leaves a rune where
/// it lands, which bursts `ticks` later: a timed effect on the caster (ADR
/// 0006), so `ModifyDuration` hastens or delays it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delay {
    pub ticks: u64,
}

/// Effect part: a delayed spell waiting on the ground, its damage fixed when
/// it was cast. Bursts over `area` on expiry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blast {
    pub min: i32,
    pub max: i32,
    pub bonus: i32,
    pub types: DamageTypes,
    pub area: Area,
}

/// Spell part: a bolt that, having hit, leaps on to the nearest enemy not
/// yet hit within `reach` tiles, up to `jumps` times. Each creature takes its
/// own roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chain {
    pub jumps: u32,
    pub reach: i32,
}

/// Spell part: calls `count` creatures of `blueprint` onto the free tiles
/// nearest the aimed spot, for `lifetime` ticks, on the caster's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summon {
    pub blueprint: BlueprintId,
    pub count: u32,
    pub lifetime: u64,
}

/// A summoned creature's tie to whoever called it: it fights on their side
/// and is dismissed when they die.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Summoned {
    pub owner: Entity,
}

/// Effect part: the timed effect holding a summon in the world. When it
/// expires, its target is dismissed (ADR 0006: the lifetime is `Timed`, so
/// `ModifyDuration` lengthens or shortens it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding;

/// Caster part: no more than `max` of the caster's summons at once. A cast
/// that would go over is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummonLimit {
    pub max: u32,
}

/// A spell that was just cast and cannot be cast again until `Timed` ends.
/// Read from the timestamps, never from a timer (ADR 0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recharging(pub Timed);

/// Caster part, the cooldown discipline: spells of `school` cost no resource,
/// they recharge for `power * ticks_per_power` after each cast, and scale
/// with `stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CooldownCasting {
    pub school: School,
    pub ticks_per_power: u64,
    pub stat: Stat,
}

/// Caster part, the mana discipline: spells of `school` cost
/// `power * mana_per_power` from the caster's `Mana`, and scale with `stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaCasting {
    pub school: School,
    pub mana_per_power: u32,
    pub stat: Stat,
}

/// Caster part, blood magic: spells of `school` cost `power * hp_per_power`
/// of the caster's own health, which they can't pay with their last point;
/// they scale with `stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BloodCasting {
    pub school: School,
    pub hp_per_power: i32,
    pub stat: Stat,
}

/// Spell part: the spell is a scroll, cast by reading it. It needs no
/// discipline and no stat, and reading it uses it up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scroll;

/// Equipment part: a focus for `school`, adding `range` tiles, `power` (a
/// negative one makes casts cheaper, never below 0) and `damage` to every
/// cast of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Focus {
    pub school: School,
    #[serde(default)]
    pub range: i32,
    #[serde(default)]
    pub power: i32,
    #[serde(default)]
    pub damage: i32,
}

/// Caster part: casting can fizzle, after the price is paid. The chance, out
/// of 10 000, is `base_bp` plus `per_armor_bp` for every point of armor worn,
/// less `per_modifier_bp` for every point of the `stat` modifier; never
/// below 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fizzle {
    pub base_bp: u32,
    pub per_armor_bp: u32,
    pub per_modifier_bp: u32,
    pub stat: Stat,
}

/// How one stat grows a pool, per point of its holder's modifier for it:
/// `max` more capacity and `regen_bp` faster regeneration (10 000 =
/// twice the pace). A negative modifier shrinks both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolGrowth {
    pub stat: Stat,
    #[serde(default)]
    pub max: i32,
    #[serde(default)]
    pub regen_bp: i32,
}

/// A pool's size and pace for one holder: its base grown by their stats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolRates {
    pub max: u32,
    /// Ticks per point regained; 0 never regains.
    pub regen_ticks: u64,
}

/// However poor the stats, regeneration keeps a quarter of its base pace.
const SLOWEST_PACE_BP: i64 = 2_500;
const BASE_PACE_BP: i64 = 10_000;

/// A pool of something spent and regained (mana, stamina). `value` was the
/// pool at `as_of`; it regains a point every `regen_ticks` ticks since, up to
/// `max`, computed on read so no timer runs per tick (ADR 0006). `max` and
/// `regen_ticks` are the base, which `growth` scales by the holder's stats
/// (`rates`). A `value` above the maximum reads as full, so a blueprint can
/// start a pool full whatever the stats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pool {
    pub value: u32,
    pub max: u32,
    #[serde(default)]
    pub as_of: Tick,
    pub regen_ticks: u64,
    #[serde(default)]
    pub growth: Vec<PoolGrowth>,
}

impl Pool {
    /// Capacity and pace for a holder with `stats`. Stats are read now, for
    /// the whole stretch since `as_of`: anything that changes a stat should
    /// bank the pool first (`spend(0, ..)`).
    pub fn rates(&self, stats: Option<Stats>) -> PoolRates {
        let modifier = |stat| i64::from(stats.map_or(0, |stats| stats.modifier(stat)));
        let grown = |per: fn(&PoolGrowth) -> i32| -> i64 {
            self.growth
                .iter()
                .map(|growth| i64::from(per(growth)) * modifier(growth.stat))
                .sum()
        };
        let max = (i64::from(self.max) + grown(|growth| growth.max)).max(0);
        let pace = (BASE_PACE_BP + grown(|growth| growth.regen_bp)).max(SLOWEST_PACE_BP);
        let regen_ticks = match self.regen_ticks {
            0 => 0,
            base => {
                let ticks = u128::from(base) * BASE_PACE_BP as u128 / pace as u128;
                u64::try_from(ticks).unwrap_or(u64::MAX).max(1)
            }
        };
        PoolRates {
            max: u32::try_from(max).unwrap_or(u32::MAX),
            regen_ticks,
        }
    }

    fn regained(&self, rates: PoolRates, now: Tick) -> u64 {
        now.since(self.as_of)
            .checked_div(rates.regen_ticks)
            .unwrap_or(0)
    }

    pub fn current(&self, rates: PoolRates, now: Tick) -> u32 {
        let regained = u32::try_from(self.regained(rates, now)).unwrap_or(u32::MAX);
        self.value.saturating_add(regained).min(rates.max)
    }

    /// The pool after spending `amount` at `now`. Progress toward the next
    /// point carries over, unless the pool was full and so not regaining.
    pub fn spend(&self, amount: u32, rates: PoolRates, now: Tick) -> Self {
        let current = self.current(rates, now);
        let as_of = if current == rates.max {
            now
        } else {
            self.as_of
                .after(self.regained(rates, now) * rates.regen_ticks)
        };
        Self {
            value: current.saturating_sub(amount),
            as_of,
            ..self.clone()
        }
    }
}

/// Caster part: the mana pool spells of a `ManaCasting` school are paid from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Mana(pub Pool);

/// Creature part: the stamina pool techniques are paid from (ADR 0024). No
/// blueprint has one yet and every technique costs 0, so stamina is off
/// until content turns it on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Stamina(pub Pool);

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
    /// Tiles it holds steady; past them each tile costs accuracy
    /// (docs/plans/fights.md). A crude bow holds less.
    #[serde(default = "default_steady")]
    pub steady: i32,
}

fn default_steady() -> i32 {
    4
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

/// Where a creature last perceived an enemy (docs/plans/fights.md): it goes
/// there when it loses sight, so hiding is a lure, and forgets on arrival.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quarry {
    pub at: (i32, i32),
}

/// Technique part: performing it only winds up; the blow lands `ticks`
/// later on the tiles it marked, and a hit before then staggers it
/// (docs/plans/fights.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindUp {
    pub ticks: u64,
}

/// Effect part: a wound-up `technique` that lands on `tiles` when the effect
/// expires. Shown on the map, so stepping out of it is a choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Telegraph {
    pub technique: Entity,
    pub tiles: Vec<(i32, i32)>,
}

/// When a creature last stepped (docs/plans/fights.md): with `Loosed`, what
/// tells a steady shot from a snap shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Strode {
    pub at: Tick,
}

/// When a creature last shot a launcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loosed {
    pub at: Tick,
}

/// A creature that keeps its distance (docs/plans/fights.md): with an enemy
/// beside it, it steps away when it can, and fights only when cornered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skirmish;

/// Sprinting (`sprint`): steps cost half the energy and a point of stamina,
/// and attacking is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sprinting;

/// A creature that sprints while chasing from afar (`sprint::decide`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SprintAi;

/// A creature that patrols the floor when it has no quarry: it walks to the
/// farthest open ground it can reach, then picks the farthest from there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roams;

/// Where a roamer is walking to (`Roams`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub to: (i32, i32),
}

/// Below `flee_below` percent of its max health a creature steps away from
/// the nearest enemy, and fights only when cornered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Morale {
    pub flee_below: u32,
}

/// Max health grows with Toughness (docs/plans/sturdiness.md): the base
/// (`BaseHealth`) plus `per_level` for each level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hardiness {
    pub per_level: i32,
}

/// Max health before Toughness: what the blueprint gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseHealth(pub i32);

/// Health lost to enemies on this floor and not yet healed back: what
/// regeneration repays, training Recovery (docs/plans/sturdiness.md).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wounds {
    pub owed: u32,
}

/// Health regained over time (docs/plans/rest.md): a point every `interval`
/// ticks while the floor's `RestBudget` lasts. Entering a floor sets the
/// budget to `per_floor` percent of max health, so a floor heals only so much.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Regeneration {
    pub interval: u64,
    pub per_floor: u32,
    /// Percent of max health each tick heals (at least a point), so a rest
    /// takes as long whatever max health has grown to.
    #[serde(default = "default_share")]
    pub share: u32,
}

fn default_share() -> u32 {
    5
}

/// Health still to regain on this floor. A count, not a clock: only
/// healing spends it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestBudget {
    pub left: u32,
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
    /// Whether a graze is enough: a poisoned needle's is, a bleeding cut's
    /// is not.
    pub on_graze: bool,
}

/// Weapon part: the chance, out of 10 000, that a blow which misses still
/// grazes (ADR 0017). Forgiving weapons graze more; without it, the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graze(pub u32);

/// Equipment part: the chance, out of 10 000, to turn a graze aside
/// entirely (a buckler, a pauldron). Only the best one worn counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deflect(pub u32);

/// Equipment part: a light source, such as a torch. Held, it lets its
/// holder see out to `radius` (if that beats plain sight) and tints every
/// tile it lights `tint_bp` / 10 000 of the way toward `tint`. Only the
/// brightest one held counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Light {
    pub radius: i32,
    pub tint: Rgb,
    pub tint_bp: u32,
}

/// What a creature has learned: XP per skill, sorted by id (ADR 0017).
/// Levels are derived (`skill::level`), never stored. Built from
/// `ComponentDef::Skills` at load and saved by name, since it holds ids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Skills(pub Vec<(SkillId, u32)>);

/// Item, spell or creature part: the skills that using it trains (a knife
/// trains Dagger), on top of those the action itself implies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trains(pub Vec<SkillId>);

/// A creature whose skills grow by use: only the player (ADR 0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Learner;

/// How often a learner has trained each skill against each kind of thing
/// (its `Name`), sorted; repetition makes a use teach less (ADR 0017).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Familiarity(pub Vec<(SkillId, String, u32)>);

/// Marks a proficiency (ADR 0018): a passive qualification attached to the
/// creature that attained it, whose other parts do the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proficiency;

/// Marks an ability (ADR 0025): a technique or part that comes from what its
/// holder is, from its origin, never taught, discovered or invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ability;

/// Proficiency part: dodging perfectly is `perfect_bp` (out of 10 000) more
/// likely when the holder is attacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footwork {
    pub perfect_bp: u32,
}

/// The discoveries a learner has made, by name, sorted (ADR 0018).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovered(pub Vec<String>);

/// Language part, beside `Name`: the verb a thing's use is logged with,
/// conjugated for whoever does it: "trip" reads "You trip the goblin." and
/// "The goblin trips you." Kept apart from what the thing does, so its data
/// describes only that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verb {
    pub word: String,
    /// Whether it takes its object with "at": "You lunge at the goblin."
    #[serde(default)]
    pub at: bool,
}

/// The blueprint a thing was made from, by name, set by every spawn from a
/// blueprint. What outlives the world names things by it (a legacy record,
/// docs/plans/legacy.md), never by `BlueprintId`, which is runtime-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Made(pub String);

/// Language part: what looking at a thing reads, beyond its name ("An
/// abandoned camp, left years ago.").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note(pub String);

/// Marker: an item made for one place, such as a predecessor's writings in
/// their ruins (docs/plans/legacy.md), so it is never found lying about on
/// a floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keepsake;

/// Item part (docs/plans/legacy.md): studying it raises each skill to at
/// most this level, never lowering one. By skill name, like the record it
/// is written from; a name the content no longer has is skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memoir(pub Vec<(String, u32)>);

/// Item part (docs/plans/legacy.md): studying it reveals these discoveries,
/// by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reveals(pub Vec<String>);

/// The discoveries a learner has read of (`Reveals`), by name, sorted:
/// `discovery::ideas` lists each with all its requirements, met or not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revealed(pub Vec<String>);

/// An item left on the camp floor while the player is away
/// (docs/plans/legacy.md): its spot, in place of a `Position`, so leaving
/// camp doesn't forget it and nothing on a floor sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stashed(pub Position);

/// Where a character started (docs/plans/origins.md): the key of its origin
/// in `data/origins.ron`. Saved by key; legacy will read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin(pub String);

/// A style (ADR 0019): a practice its holder follows. A known style is
/// carried (`Inventory`), inert; the one followed is attached, so its
/// principles take part in events. One followed per domain; a hybrid fills
/// every domain it lists. Built from `StyleDef` at load (it holds ids) and
/// saved by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Style {
    pub domains: Vec<crate::skill::Domain>,
    /// Its own skill, trained by uses that follow it.
    pub skill: SkillId,
    /// A use follows the style when it uses all of these.
    pub follows: Vec<SkillId>,
    /// ... and the wielded weapon has all of these (`style::following`).
    pub wielding: Vec<SkillId>,
    pub principles: Vec<Principle>,
    /// Techniques it invents at levels of its skill (ADR 0024).
    pub invents: Vec<(u32, crate::blueprint::BlueprintId)>,
    /// Phases it thrives in, by key, each with how much more style XP a use
    /// following it gains on a dive there, in percent (docs/plans/phases.md).
    pub thrives: Vec<(String, u32)>,
}

/// What a style does once its skill reaches `from_level`; nothing before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principle {
    pub from_level: u32,
    pub does: Does,
}

/// The effects a principle can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Does {
    /// After a perfect dodge, strike back at once, for free.
    Riposte,
    /// Shooting with an enemy alongside costs no accuracy (point blank).
    SteadyClose,
    /// With every action, arrows lying within a tile go back in the quiver.
    Gather,
}

/// Item part: studying it teaches these skills this much XP, once per kind
/// of thing (its `Name`) for each learner (ADR 0017). Built from
/// `ComponentDef::Lesson` at load and saved by name, since it holds ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lesson(pub Vec<(SkillId, u32)>);

/// Item part: studying it teaches these techniques (ADR 0024), once per
/// kind of thing, like a `Lesson`. Built from `ComponentDef::Teaches` at
/// load and saved by name, since it holds ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Teaches(pub Vec<crate::blueprint::BlueprintId>);

/// The kinds of thing (by `Name`) a learner has already studied, sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Studied(pub Vec<String>);

/// The kinds of creature (by `Name`) the player has laid eyes on, sorted:
/// each teaches Beast lore once (ADR 0017).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sighted(pub Vec<String>);

/// The spells (by `Name`) the player has cast at least once, sorted: each
/// teaches Arcana the first time (ADR 0017).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attuned(pub Vec<String>);

/// A technique (ADR 0024): a special action its holder knows, attached to
/// them like a spell. It says how it is aimed and what it asks; payload parts
/// (`Blow`, `Dash`) say what it does. Built from `TechniqueDef` at load (it
/// holds ids) and saved by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Technique {
    pub domain: crate::skill::Domain,
    /// How far away, in tiles, its target may stand.
    pub reach: i32,
    /// Ticks before it can be performed again, whatever its holder's speed.
    pub recharge: u64,
    /// Taken from the performer's `Stamina`; 0 needs no pool.
    pub stamina: u32,
    /// All must hold for it to be performed.
    pub requires: Vec<Need>,
    /// What else a full hit does, after its damage.
    pub riders: Vec<Rider>,
    /// The skills its blow uses, for its level and its training, instead of
    /// the weapon's: a technique that is not plain weapon work.
    pub uses: Option<Vec<SkillId>>,
    /// What it gains while a style is followed.
    pub bonuses: Vec<StyleBonus>,
    /// Its place in a form of several techniques performed in turn.
    pub combo: Option<Combo>,
}

/// A step of a form (ADR 0024): techniques of one `group`, performed in
/// order of `step`, each within `within` ticks of the one before. A step
/// that continues the run gains its bonus, and every blow of a run trains
/// `xp_pct` percent more per step already in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combo {
    pub group: String,
    pub step: u32,
    pub within: u64,
    pub accuracy: i32,
    pub damage_pct: i32,
    pub riders: Vec<Rider>,
    pub xp_pct: u32,
    /// Read from the first step.
    pub display: ComboDisplay,
}

/// How the technique list shows a form.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComboDisplay {
    /// Every step, each its own row.
    #[default]
    Steps,
    /// One row: the step that would come next.
    Turns,
}

/// Creature part: the last step of a form it performed, when, and how many
/// steps the run has had (ADR 0024). Read against `within` on the next step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Performed {
    pub group: String,
    pub step: u32,
    pub at: crate::time::Tick,
    pub run: u32,
}

/// What a technique gains while the style whose skill is `style` is
/// followed, from `from_level` of that skill (ADR 0024): its numbers, its
/// price, more riders. Percentages add to the base (negative takes away).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleBonus {
    pub style: SkillId,
    pub from_level: u32,
    pub accuracy: i32,
    pub damage_pct: i32,
    pub recharge_pct: i32,
    pub stamina_pct: i32,
    pub riders: Vec<Rider>,
}

/// What a technique's full hit does besides its damage (ADR 0024). Data, so
/// a style's bonus can add one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rider {
    /// Pushes the target straight away from the performer, up to `tiles`,
    /// stopping at walls and creatures.
    Shove { tiles: i32 },
    /// Applies a timed effect to the target (`ApplyEffect`).
    Effect(crate::blueprint::BlueprintId),
}

/// Effect part: its target is reeling, so blows at it land more often by
/// `hit_bp` and it can't dodge perfectly. Active while attached; its expiry
/// is checked against its timestamps like any effect's (ADR 0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffBalance {
    pub hit_bp: u32,
}

/// Something a technique asks of its performer. Each is checked in one place
/// (`technique::unmet`), with its own refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Need {
    /// The blow uses all of these skills: what is wielded and how (a
    /// thrusting weapon, an empty off hand).
    Wielding(Vec<SkillId>),
    /// At least this level of a skill.
    Skill(SkillId, u32),
    /// The style whose skill this is is followed.
    Following(SkillId),
    /// Only as the next step of a run of its form (`Combo`).
    Continuing,
    /// A body part of this kind: feet to kick with.
    Part(PartKind),
}

/// Technique part: it strikes with what its performer wields, shifted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blow {
    #[serde(default)]
    pub accuracy: i32,
    /// Percent added to the damage rolled; negative takes it away.
    #[serde(default)]
    pub damage_pct: i32,
    /// Struck with the body (a kick), not what is wielded: the creature's
    /// natural `Melee`, trained as an unarmed blow, whatever is in hand.
    #[serde(default)]
    pub unarmed: bool,
    /// Percent added to the damage per level of the blow's skill: a form
    /// rewards understanding far more than a formless bump does.
    #[serde(default)]
    pub mastery: i32,
}

/// Technique part: the performer first moves up to `tiles` along the line to
/// its target, stopping beside it; or, `away`, steps back that far from it,
/// needing open, empty ground behind (a shot then flies from there).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dash {
    pub tiles: i32,
    #[serde(default)]
    pub away: bool,
}

/// Technique part: it shoots the wielded launcher's next arrow (ADR 0013)
/// instead of striking, shifted like a blow. The shot is an ordinary shot:
/// its flight, odds, breaking and training are `ranged`'s.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loose {
    #[serde(default)]
    pub accuracy: i32,
    /// Percent added to its damage; negative takes it away.
    #[serde(default)]
    pub damage_pct: i32,
}
