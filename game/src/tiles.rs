//! Sprites (ADR 0023): `data/tiles.ron` names cells of an atlas image, one or more frames each, and which sprite each terrain uses; an `Appearance` names its sprite. Loaded and checked with the blueprints, so a missing name fails at load, never in play. Drawing them, and the clock that turns frames, is a frontend's; `term` shows glyphs only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::blueprint::{BlueprintError, ron_options};
use crate::map::Tile;

/// A sprite, by its place in `Tiles`. Assigned at load, never saved:
/// `Appearance` keeps the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpriteId(u16);

/// A sprite's frames, as cells of the atlas (column, row), shown
/// `frame_ms` each in turn; one frame means it stands still.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Sprite {
    pub frames: Vec<(u32, u32)>,
    #[serde(default)]
    pub frame_ms: u32,
}

impl Sprite {
    /// The atlas cell to show `elapsed_ms` into the animation.
    pub fn frame(&self, elapsed_ms: u64) -> (u32, u32) {
        let count = self.frames.len().max(1) as u64;
        let index = match self.frame_ms {
            0 => 0,
            ms => (elapsed_ms / u64::from(ms)) % count,
        };
        self.frames
            .get(usize::try_from(index).unwrap_or(0))
            .copied()
            .unwrap_or((0, 0))
    }
}

/// Which sprite each kind of terrain uses, if any.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct TerrainDef {
    floor: Option<String>,
    wall: Option<String>,
    down: Option<String>,
    up: Option<String>,
    #[serde(default)]
    door: Option<String>,
    #[serde(default)]
    door_open: Option<String>,
    #[serde(default)]
    chasm: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TilesDef {
    atlas: PathBuf,
    cell: (u32, u32),
    #[serde(default)]
    terrain: TerrainDef,
    sprites: BTreeMap<String, Sprite>,
}

/// The sprite table. Empty when content has none; then every frontend
/// shows glyphs.
#[derive(Debug, Clone, Default)]
pub struct Tiles {
    /// The atlas image, resolved against the data directory.
    pub atlas: PathBuf,
    /// One atlas cell in pixels.
    pub cell: (u32, u32),
    sprites: Vec<Sprite>,
    ids: BTreeMap<String, SpriteId>,
    floor: Option<SpriteId>,
    wall: Option<SpriteId>,
    down: Option<SpriteId>,
    up: Option<SpriteId>,
    door: Option<SpriteId>,
    door_open: Option<SpriteId>,
    chasm: Option<SpriteId>,
}

impl Tiles {
    /// Reads `tiles.ron` from `data`. Every sprite needs a frame, and
    /// terrain may only name sprites listed.
    pub(crate) fn load(data: &Path) -> Result<Self, BlueprintError> {
        let path = data.join("tiles.ron");
        let text =
            std::fs::read_to_string(&path).map_err(|err| BlueprintError::Io(path.clone(), err))?;
        let def: TilesDef = ron_options()
            .from_str(&text)
            .map_err(|err| BlueprintError::Parse(path.clone(), Box::new(err)))?;
        let mut tiles = Self {
            atlas: data.join(def.atlas),
            cell: def.cell,
            ..Self::default()
        };
        for (name, sprite) in def.sprites {
            if sprite.frames.is_empty() {
                return Err(BlueprintError::SpriteWithoutFrames(name));
            }
            let id = SpriteId(
                u16::try_from(tiles.sprites.len()).map_err(|_| BlueprintError::TooManySprites)?,
            );
            tiles.sprites.push(sprite);
            tiles.ids.insert(name, id);
        }
        let terrain = |name: Option<String>| {
            name.map(|name| {
                tiles.id(&name).ok_or(BlueprintError::UnknownSprite {
                    owner: "terrain".into(),
                    sprite: name,
                })
            })
            .transpose()
        };
        let chasm = terrain(def.terrain.chasm)?;
        let (floor, wall, down, up, door, door_open) = (
            terrain(def.terrain.floor)?,
            terrain(def.terrain.wall)?,
            terrain(def.terrain.down)?,
            terrain(def.terrain.up)?,
            terrain(def.terrain.door)?,
            terrain(def.terrain.door_open)?,
        );
        tiles.floor = floor;
        tiles.wall = wall;
        tiles.down = down;
        tiles.up = up;
        tiles.door = door;
        tiles.door_open = door_open;
        tiles.chasm = chasm;
        Ok(tiles)
    }

    pub fn id(&self, name: &str) -> Option<SpriteId> {
        self.ids.get(name).copied()
    }

    pub fn get(&self, id: SpriteId) -> Option<&Sprite> {
        self.sprites.get(usize::from(id.0))
    }

    pub(crate) fn terrain(&self, tile: Tile) -> Option<SpriteId> {
        match tile {
            Tile::Floor => self.floor,
            Tile::Wall => self.wall,
            Tile::StairsDown => self.down,
            Tile::StairsUp => self.up,
            Tile::Door => self.door,
            Tile::DoorOpen => self.door_open,
            Tile::Chasm => self.chasm,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")
    }

    #[test]
    fn the_shipped_table_loads_and_names_its_terrain() {
        let tiles = Tiles::load(&data()).unwrap();
        assert!(tiles.atlas.ends_with("tiles.png"));
        assert!(tiles.atlas.exists(), "the atlas ships beside the table");
        let floor = tiles.terrain(Tile::Floor).unwrap();
        assert_eq!(Some(floor), tiles.id("floor"));
        assert!(!tiles.get(floor).unwrap().frames.is_empty());
    }

    #[test]
    fn a_new_game_shows_sprites_in_view_and_glyphs_from_memory() {
        let data = data();
        // Without the camp: a floor larger than the view, so the camera
        // follows the player.
        let blueprints = crate::blueprint::Blueprints::load(&data)
            .unwrap()
            .with_camp(None);
        let mut state = crate::state::GameState::new(3, blueprints).unwrap();
        let tiles = state.blueprints.tiles().clone();
        let view = crate::state::render_view(&state, 40, 20);
        let centre = view.get(20, 10).unwrap();
        assert_eq!(centre.glyph, '☺', "the camera follows the player");
        assert_eq!(centre.sprite, tiles.id("player"));

        // Forget the view: what was seen is remembered, as glyphs.
        state.visible = crate::map::Grid::new(state.map.width, state.map.height, false);
        let view = crate::state::render_view(&state, 40, 20);
        assert!((0..20).all(|y| (0..40).all(|x| view.get(x, y).unwrap().sprite.is_none())));
    }

    #[test]
    fn a_blueprint_naming_an_unknown_sprite_fails_at_load() {
        use crate::blueprint::{Blueprint, Blueprints, ComponentDef};
        use crate::cell::Rgb;
        use crate::entity::Appearance;
        let raw = BTreeMap::from([(
            "Ghost".to_string(),
            Blueprint {
                inherits: None,
                components: vec![ComponentDef::Appearance(Appearance {
                    glyph: 'G',
                    fg: Rgb(1, 1, 1),
                    bg: Rgb(0, 0, 0),
                    sprite: Some("ghost".into()),
                })],
                equipment: None,
                spells: None,
                techniques: None,
            },
        )]);
        let loaded = Blueprints::resolve(raw)
            .unwrap()
            .with_tiles(Tiles::load(&data()).unwrap());
        assert!(matches!(
            loaded,
            Err(BlueprintError::UnknownSprite { owner, sprite }) if owner == "Ghost" && sprite == "ghost"
        ));
    }

    #[test]
    fn frames_turn_over_on_the_clock() {
        let sprite = Sprite {
            frames: vec![(0, 0), (1, 0), (2, 0)],
            frame_ms: 100,
        };
        assert_eq!(sprite.frame(0), (0, 0));
        assert_eq!(sprite.frame(150), (1, 0));
        assert_eq!(sprite.frame(320), (0, 0));
        let still = Sprite {
            frames: vec![(4, 2), (5, 2)],
            frame_ms: 0,
        };
        assert_eq!(still.frame(10_000), (4, 2));
    }

    fn load(text: &str) -> Result<Tiles, BlueprintError> {
        let dir = std::env::temp_dir().join(format!("roguelike-tiles-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tiles.ron"), text).unwrap();
        let loaded = Tiles::load(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        loaded
    }

    #[test]
    fn a_bad_table_fails_at_load() {
        let unknown =
            load(r#"(atlas: "a.png", cell: (8, 16), terrain: (wall: "brick"), sprites: {})"#);
        assert!(
            matches!(unknown, Err(BlueprintError::UnknownSprite { sprite, .. }) if sprite == "brick")
        );
        let empty = load(r#"(atlas: "a.png", cell: (8, 16), sprites: { "x": (frames: []) })"#);
        assert!(matches!(empty, Err(BlueprintError::SpriteWithoutFrames(name)) if name == "x"));
    }
}
