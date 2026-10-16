//! Test-only additions to `spawn::Spawn`. Tests describe an ad-hoc entity by
//! what it is (`.item()`, `.armor(2)`, `.equipped_on(owner, 2)`), so a change
//! to how a component or a holding is stored is made here once, not in every
//! test.

use hecs::Entity;

use crate::cell::Rgb;
use crate::entity::{
    Appearance, Armor, Energy, Equippable, Found, Health, Hostile, Item, Melee, Name, PartKind,
    SpawnSeq, Speed, Stat, Stats, Weapon,
};
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
    pub(crate) fn spawn(self, state: &mut GameState) -> Entity {
        state.spawn(self).expect("test spawn")
    }

    pub(crate) fn look(self, glyph: char, fg: Rgb) -> Self {
        self.with(Appearance {
            glyph,
            fg,
            bg: Rgb(0, 0, 0),
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
        self.with(Weapon { min, max, stat })
    }

    pub(crate) fn armor(self, av: i32) -> Self {
        self.with(Armor { av })
    }

    /// At full health.
    pub(crate) fn health(self, max: i32) -> Self {
        self.with(Health { current: max, max })
    }

    pub(crate) fn melee(self, min: i32, max: i32) -> Self {
        self.with(Melee { min, max })
    }

    pub(crate) fn stats(self, strength: i32, agility: i32) -> Self {
        self.with(Stats { strength, agility })
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

    /// In `owner`'s inventory.
    pub(crate) fn carried_by(mut self, owner: Entity) -> Self {
        self.held = Some(Held::Carried(owner));
        self
    }
}
