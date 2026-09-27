//! Simulation and game logic. This crate must never depend on a
//! terminal/graphics crate (crossterm, ratatui, etc.) — that boundary is
//! what makes the rendering seam real instead of aspirational.

pub mod cell;
pub mod input;

pub use cell::{Cell, Rgb};
pub use input::{Dir, InputEvent};

pub mod map;

pub use map::{generate_map, random_seed, Grid, Tile, MAP_HEIGHT, MAP_WIDTH};

pub mod blueprint;

pub use blueprint::{load_blueprint, BlueprintError, EntityBlueprint};

pub mod command;
pub mod entity;
pub mod state;

pub use command::{apply_commands, Command};
pub use entity::{Appearance, BlocksMovement, Position};
pub use state::{render_view, GameState, MessageLog};
