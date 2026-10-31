//! What every frontend shares: the keyboard flows (a `Session` and its modes), the title menu, and save files. No drawing and no key bindings, so a terminal and a window behave the same and only differ in how they look.

pub mod input;
pub mod menu;
pub mod overlay;
pub mod saves;
pub mod session;
pub mod targeting;
pub mod text;
pub mod title;

#[cfg(test)]
pub(crate) mod tests {
    use std::path::{Path, PathBuf};

    use game::{Blueprints, GameState};

    /// The content every `front` test loads.
    pub fn data() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")
    }

    /// A new game from the test content, seed 3.
    pub fn state() -> GameState {
        state_as("Wanderer")
    }

    /// `state`, starting as another origin (`data/origins.ron`).
    pub fn state_as(origin: &str) -> GameState {
        let blueprints = Blueprints::load(&data()).unwrap();
        GameState::new_as(3, blueprints, Some(origin)).unwrap()
    }
}
