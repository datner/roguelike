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

pub use zone::{CaveParams, ZONE_HEIGHT, ZONE_WIDTH, ZoneCoord, generate_cave, zone_seed};

pub mod blueprint;

pub use blueprint::{
    Blueprint, BlueprintError, BlueprintId, Blueprints, ComponentDef, OnHitDef, ResolvedBlueprint,
};

pub mod ai;
pub mod aim;
pub mod ammo;
pub mod area;
pub mod camera;
pub mod camp;
pub mod combat;
pub mod command;
pub mod cost;
pub mod discovery;
pub mod dive;
pub mod dungeon;
pub mod effect;
pub mod entity;
pub mod equipment;
pub mod event;
pub mod explore;
pub mod fov;
pub mod fx;
pub mod grammar;
pub mod holding;
pub mod light;
pub mod lore;
pub mod phase;
pub mod prio;
pub mod ranged;
pub mod rng;
pub mod save;
pub mod side;
pub mod skill;
pub mod spawn;
pub mod spell;
pub mod state;
pub mod style;
pub mod tiles;
pub mod time;
pub mod training;
pub mod view;

#[cfg(test)]
mod testkit;

pub use aim::{Aim, Chances, FireStep, Shot, aim, aim_range, aim_targets, fire_step};
pub use camera::{Camera, camera};
pub use camp::Camp;
pub use command::MAX_COMMANDS_PER_STEP;
pub use discovery::{DiscoveryDef, Grant, IdeaView, Requirement, ideas, proficiencies};
pub use dive::{Place, PlaceView, place_view};
pub use dungeon::{Dungeon, Inhabitant};
pub use effect::DurationChange;
pub use entity::{
    Ammo, AmmoKind, Appearance, Area, Armor, Bleeding, BlocksMovement, Body, BodyPart, DamageType,
    DamageTypes, Dead, Duration, Energy, Equippable, EquippedOn, Found, Health, Hits, Hostile,
    Item, Launcher, Melee, Name, OnHit, OnHitEntry, PartKind, Player, Position, Projectile, Quiver,
    Shape, SpawnSeq, Speed, Stat, Stats, Throwable, Timed, Weapon,
};
pub use explore::{ExploreItems, cycle_explore_items, explore_step, travel_step};
pub use fx::{Fx, Outcome, take_fx};
pub use grammar::{Subject, capitalize};
pub use light::{Lit, render_layers, tint};
pub use lore::{Inspection, inspect, look_targets};
pub use phase::{Phase, PhaseView, phases, wait_refusal};
pub use ranged::Trail;
pub use rng::Rngs;
pub use save::{LoadError, SAVE_VERSION, SaveError};
pub use skill::{Domain, SkillBook, SkillDef};
pub use state::{GameState, MessageLog, render_view};
pub use style::{StyleDef, StyleView, styles};
pub use tiles::{Sprite, SpriteId, Tiles};
pub use time::{Tick, Timers, advance, take_turn};
pub use training::practice_step;
pub use view::{
    CastPreview, CharacterSheet, CostLine, ItemRef, ItemView, PartView, SkillView, SpellView,
    character_sheet, equipment_view, inventory_view, skills, spells, throwables,
};
