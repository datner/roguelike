//! Simulation and game logic. This crate must never depend on a
//! terminal/graphics crate (crossterm, ratatui, etc.) — that boundary is
//! what makes the rendering seam real instead of aspirational.

pub mod cell;
pub mod input;

pub use cell::{Cell, Rgb};
pub use input::{Dir, InputEvent, PlayerAction};

pub mod map;

pub use map::{Grid, Map, Tile, random_seed};

pub mod zone;

pub use zone::{
    CaveParams, CaveParamsError, ZONE_HEIGHT, ZONE_WIDTH, ZoneCoord, generate_cave, zone_seed,
};

pub mod blueprint;

pub use blueprint::{
    Blueprint, BlueprintError, BlueprintId, Blueprints, ComponentDef, OnHitDef, ResolvedBlueprint,
};

pub mod ai;
pub mod aim;
pub mod ammo;
pub mod camera;
pub mod combat;
pub mod command;
pub mod cost;
pub mod effect;
pub mod entity;
pub mod equipment;
pub mod event;
pub mod explore;
pub mod fov;
pub mod grammar;
pub mod holding;
pub mod prio;
pub mod ranged;
pub mod rng;
pub mod save;
pub mod spawn;
pub mod spell;
pub mod state;
pub mod time;
pub mod view;

#[cfg(test)]
mod testkit;

pub use aim::{Aim, FireStep, Shot, aim, aim_range, aim_targets, fire_step};
pub use camera::{Camera, camera};
pub use command::MAX_COMMANDS_PER_STEP;
pub use effect::DurationChange;
pub use entity::{
    Ammo, AmmoKind, Appearance, Armor, Bleeding, BlocksMovement, Body, BodyPart, DamageType,
    DamageTypes, Dead, Duration, Energy, Equippable, EquippedOn, Found, Health, Hostile, Item,
    Launcher, Melee, Name, OnHit, OnHitEntry, PartKind, Player, Position, Projectile, Quiver,
    SpawnSeq, Speed, Stat, Stats, Throwable, Timed, Weapon,
};
pub use explore::{ExploreItems, cycle_explore_items, explore_step};
pub use grammar::Subject;
pub use ranged::{Trail, take_trails};
pub use rng::Rngs;
pub use save::{LoadError, SAVE_VERSION, SaveError};
pub use state::{GameState, MessageLog, render_view};
pub use time::{Tick, Timers, advance, take_turn};
pub use view::{
    CastPreview, CharacterSheet, CostLine, ItemRef, ItemView, PartView, SpellView, character_sheet,
    equipment_view, inventory_view, spells, throwables,
};
