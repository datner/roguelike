//! `Spawn`: what to create and where it goes, handed to `GameState::spawn`,
//! the only way entities are made (ADR 0008). Placement and holding are
//! described here once instead of patched on after each spawn.

use hecs::{Component, Entity, EntityBuilder};

use crate::blueprint::BlueprintId;
use crate::entity::Position;

pub(crate) enum Source {
    Blueprint(String),
    Id(BlueprintId),
    /// Nothing but what registration adds; tests build these up by hand.
    #[cfg(test)]
    Blank,
}

/// Where the entity goes once spawned.
#[derive(Clone, Copy)]
pub(crate) enum Held {
    /// In the owner's `Attachments`, not on a body part (an effect, say).
    Attached(Entity),
    #[cfg(test)]
    Equipped(Entity, u32),
    #[cfg(test)]
    Carried(Entity),
}

pub(crate) struct Spawn {
    pub(crate) source: Source,
    /// Added on top of the blueprint's, before registration.
    pub(crate) components: EntityBuilder,
    pub(crate) held: Option<Held>,
    /// Applied after registration, so they override what it assigns.
    #[cfg(test)]
    pub(crate) overrides: EntityBuilder,
}

impl Spawn {
    fn of(source: Source) -> Self {
        Self {
            source,
            components: EntityBuilder::new(),
            held: None,
            #[cfg(test)]
            overrides: EntityBuilder::new(),
        }
    }

    /// The blueprint named `name`, e.g. `"Goblin"`.
    pub(crate) fn blueprint(name: &str) -> Self {
        Self::of(Source::Blueprint(name.to_string()))
    }

    pub(crate) fn id(id: BlueprintId) -> Self {
        Self::of(Source::Id(id))
    }

    #[cfg(test)]
    pub(crate) fn blank() -> Self {
        Self::of(Source::Blank)
    }

    /// Adds `component`, replacing the blueprint's of the same type.
    pub(crate) fn with(mut self, component: impl Component) -> Self {
        self.components.add(component);
        self
    }

    pub(crate) fn at(self, x: i32, y: i32) -> Self {
        self.with(Position { x, y })
    }

    pub(crate) fn attached_to(mut self, owner: Entity) -> Self {
        self.held = Some(Held::Attached(owner));
        self
    }
}
