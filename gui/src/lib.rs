//! Windowed frontend on eframe/egui (ADR 0020): maps keys to `front`'s inputs and draws, keyboard first, the mouse a shortcut onto the same flows. No game or flow rules here.

pub mod app;
pub mod input;
pub mod map_view;
pub mod menu_view;
pub mod modes;
pub mod panels;
