//! Test-only additions to `spawn::Spawn`. Tests describe an ad-hoc entity by
//! what it is (`.item()`, `.armor(2)`, `.equipped_on(owner, 2)`), so a change
//! to how a component or a holding is stored is made here once, not in every
//! test.

use hecs::Entity;

use std::ops::ControlFlow;

use crate::cell::Rgb;
use crate::combat::{AttackOdds, CERTAIN};
use crate::entity::{
    Ammo, AmmoKind, Appearance, Area, Armor, DamageType, DamageTypes, Energy, Equippable, Found,
    Health, Hits, Hostile, Item, Launcher, Melee, Name, PartKind, Projectile, Quiver, School,
    Shape, SpawnSeq, Speed, Spell, SpellDamage, Stat, Stats, Summoned, Throwable, Weapon,
};
use crate::event::{Ctx, EventRegistry};
use crate::prio;
use crate::spawn::{Held, Spawn};
use crate::state::GameState;

/// An entity named `name` (the bare noun, as `Name` holds it).
pub(crate) fn thing(name: &str) -> Spawn {
    Spawn::blank().with(Name(name.to_string()))
}

/// An entity with nothing but what `GameState::spawn` registers.
pub(crate) fn blank() -> Spawn {
    Spawn::blank()
}

impl Spawn {
    /// `GameState::spawn`, unwrapped: a test's spawn failing is a test bug.
    pub(crate) fn make(self, state: &mut GameState) -> Entity {
        state.spawn(self).expect("test spawn")
    }

    pub(crate) fn look(self, glyph: char, fg: Rgb) -> Self {
        self.with(Appearance {
            glyph,
            fg,
            bg: Rgb(0, 0, 0),
            sprite: None,
        })
    }

    pub(crate) fn item(self) -> Self {
        self.with(Item)
    }

    pub(crate) fn found(self) -> Self {
        self.with(Found)
    }

    pub(crate) fn fits(self, part: PartKind) -> Self {
        self.with(Equippable { part })
    }

    pub(crate) fn weapon(self, min: i32, max: i32, stat: Stat) -> Self {
        self.with(Weapon {
            min,
            max,
            stat,
            types: DamageTypes::of(&[DamageType::Slashing]),
        })
    }

    pub(crate) fn throwable(
        self,
        range: i32,
        (min, max): (i32, i32),
        stat: Stat,
        types: &[DamageType],
    ) -> Self {
        self.with(Throwable {
            range,
            min,
            max,
            stat,
            types: DamageTypes::of(types),
        })
    }

    pub(crate) fn spell(self, school: School, range: i32, power: i32) -> Self {
        self.with(Spell {
            school,
            range,
            power,
        })
    }

    pub(crate) fn spell_damage(self, min: i32, max: i32) -> Self {
        self.with(SpellDamage {
            min,
            max,
            types: DamageTypes::default(),
        })
    }

    pub(crate) fn area(self, shape: Shape, hits: Hits, falloff: i32) -> Self {
        self.with(Area {
            shape,
            hits,
            falloff,
        })
    }

    pub(crate) fn launcher(self, range: i32, ammo: AmmoKind, stat: Stat) -> Self {
        self.with(Launcher { range, ammo, stat })
    }

    pub(crate) fn quiver(self, ammo: AmmoKind, capacity: u32) -> Self {
        self.with(Quiver { ammo, capacity })
    }

    pub(crate) fn ammo(self, kind: AmmoKind) -> Self {
        self.with(Ammo(kind))
    }

    pub(crate) fn projectile(
        self,
        min: i32,
        max: i32,
        types: &[DamageType],
        break_bp: u32,
    ) -> Self {
        self.with(Projectile {
            min,
            max,
            types: DamageTypes::of(types),
            break_bp,
        })
    }

    pub(crate) fn armor(self, av: i32) -> Self {
        self.with(Armor::uniform(av))
    }

    /// At full health.
    pub(crate) fn health(self, max: i32) -> Self {
        self.with(Health { current: max, max })
    }

    pub(crate) fn melee(self, min: i32, max: i32) -> Self {
        self.with(Melee { min, max })
    }

    pub(crate) fn stats(self, strength: i32, agility: i32) -> Self {
        self.with(Stats {
            strength,
            agility,
            ..Stats::AVERAGE
        })
    }

    pub(crate) fn summoned_by(self, owner: Entity) -> Self {
        self.with(Summoned { owner })
    }

    pub(crate) fn hostile(self) -> Self {
        self.with(Hostile)
    }

    /// Registration gives it `Energy(0)`, as for any actor.
    pub(crate) fn speed(self, speed: u32) -> Self {
        self.with(Speed(speed))
    }

    pub(crate) fn energy(mut self, energy: i64) -> Self {
        self.overrides.add(Energy(energy));
        self
    }

    /// Replaces the registered `SpawnSeq`, for scheduler tie-break tests.
    pub(crate) fn seq(mut self, seq: u64) -> Self {
        self.overrides.add(SpawnSeq(seq));
        self
    }

    /// Equipped by `owner` on body part `part`.
    pub(crate) fn equipped_on(mut self, owner: Entity, part: u32) -> Self {
        self.held = Some(Held::Equipped(owner, part));
        self
    }
}

/// The newest log message, or `""` if the log is empty.
pub(crate) fn last_log(state: &GameState) -> &str {
    state.log.recent(1).next().map_or("", String::as_str)
}

/// Everything on the map is in the player's view, for tests that aim.
pub(crate) fn see_everything(state: &mut GameState) {
    let (width, height) = (state.visible.width, state.visible.height);
    for y in 0..height {
        for x in 0..width {
            state.visible.set(x, y, true);
        }
    }
}

/// Test part: every blow its holder gives or takes lands, for tests about
/// what a hit does rather than whether it lands (ADR 0017).
#[derive(Debug, Clone, Copy)]
pub(crate) struct SureHits;

pub(crate) fn register(events: &mut EventRegistry) {
    events.on(
        prio::SURE_HITS,
        |_: &mut Ctx<'_>, _: &SureHits, mut ev: AttackOdds| {
            ev.hit_bp = CERTAIN;
            ControlFlow::Continue(ev)
        },
    );
}

/// Makes `entity`'s blows, given and taken, land (`SureHits`).
pub(crate) fn sure_hits(state: &mut GameState, entity: Entity) {
    state
        .world
        .insert_one(entity, SureHits)
        .expect("entity exists");
}
