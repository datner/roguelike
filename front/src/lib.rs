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
    use std::path::Path;

    use game::{Blueprints, CaveParams, GameState};

    /// A new game from the real data, seed 3.
    pub fn state() -> GameState {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = Blueprints::load(&data).unwrap();
        let cave = CaveParams::load(&data.join("cave.ron")).unwrap();
        GameState::new(3, blueprints, &cave).unwrap()
    }
}
