//! Test-only entity builder. Tests describe an ad-hoc entity by what it is
//! (`.item()`, `.armor(2)`, `.equipped_on(owner, 2)`), so a change to how a
//! component or a holding is stored is made here once, not in every test.

use hecs::{Component, Entity, EntityBuilder};

use crate::cell::Rgb;
use crate::entity::{
    Appearance, Armor, Energy, Equippable, Found, Health, Hostile, Item, Melee, Name, PartKind,
    Position, SpawnSeq, Speed, Stat, Stats, Weapon,
};
use crate::holding;
use crate::state::GameState;

/// An entity named `name` (the bare noun, as `Name` holds it).
pub(crate) fn thing(name: &str) -> Spawn {
    Spawn::default().with(Name(name.to_string()))
}

/// An entity with nothing but what `GameState` registers.
pub(crate) fn blank() -> Spawn {
    Spawn::default()
}

/// Where the entity goes once spawned.
#[derive(Clone, Copy)]
enum Held {
    Attached(Entity),
    Equipped(Entity, u32),
    Carried(Entity),
}

#[derive(Default)]
pub(crate) struct Spawn {
    components: EntityBuilder,
    /// Applied after registration, so they override what it assigns.
    overrides: EntityBuilder,
    held: Option<Held>,
}

impl Spawn {
    /// Escape hatch for a component without a method here.
    pub(crate) fn with(mut self, component: impl Component) -> Self {
        self.components.add(component);
        self
    }

    pub(crate) fn at(self, x: i32, y: i32) -> Self {
        self.with(Position { x, y })
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

    /// In `owner`'s `Attachments`, not on a body part (an effect, say).
    pub(crate) fn attached_to(mut self, owner: Entity) -> Self {
        self.held = Some(Held::Attached(owner));
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

    pub(crate) fn spawn(mut self, state: &mut GameState) -> Entity {
        let entity = state.spawn_built(&mut self.components);
        let world = &mut state.world;
        world
            .insert(entity, self.overrides.build())
            .expect("just spawned");
        match self.held {
            Some(Held::Attached(owner)) => holding::attach(world, owner, entity),
            Some(Held::Equipped(owner, part)) => holding::put_on(world, owner, entity, part),
            Some(Held::Carried(owner)) => holding::stow(world, owner, entity),
            None => {}
        }
        entity
    }
}
