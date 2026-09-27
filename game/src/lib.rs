//! Simulation and game logic. This crate must never depend on a
//! terminal/graphics crate (crossterm, ratatui, etc.) — that boundary is
//! what makes the rendering seam real instead of aspirational.

pub mod cell;
pub mod input;

pub use cell::{Cell, Rgb};
pub use input::{Dir, InputEvent, PlayerAction};

pub mod map;

pub use map::{Grid, MAP_HEIGHT, MAP_WIDTH, Map, Tile, generate_map, random_seed};

pub mod blueprint;

pub use blueprint::{
    Blueprint, BlueprintError, BlueprintId, Blueprints, ComponentDef, OnHitDef, ResolvedBlueprint,
};

pub mod ai;
pub mod combat;
pub mod command;
pub mod cost;
pub mod effect;
pub mod entity;
pub mod event;
pub mod fov;
pub mod grammar;
pub mod prio;
pub mod rng;
pub mod save;
pub mod state;
pub mod time;

pub use command::{MAX_COMMANDS_PER_STEP, apply_commands};
pub use effect::DurationChange;
pub use entity::{
    Appearance, Armor, Bleeding, BlocksMovement, Dead, Duration, Energy, Health, Hostile, Melee,
    Name, OnHit, OnHitEntry, Player, Position, SpawnSeq, Speed, Timed,
};
pub use grammar::Subject;
pub use rng::Rngs;
pub use save::{LoadError, SAVE_VERSION, SaveError};
pub use state::{GameState, MessageLog, render_view};
pub use time::{Tick, Timers, advance, take_turn};
