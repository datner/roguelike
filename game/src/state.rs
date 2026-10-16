//! `GameState`, the message log, and `render_view`, which turns what the player knows into a `Grid<Cell>`.

use std::collections::VecDeque;

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::blueprint::{BlueprintError, BlueprintId, Blueprints};
use crate::camera::camera;
use crate::cell::{Cell, Rgb};
use crate::combat;
use crate::command::Command;
use crate::entity::{Appearance, Attachments, Energy, Position, SpawnSeq, Speed};
use crate::event::EventRegistry;
use crate::fov;
use crate::map::{Grid, Map, Tile, choose_spawn_position};
use crate::rng::Rngs;
use crate::time::{Tick, Timers};
use crate::zone::{CaveParams, ZoneCoord, generate_cave, start_position, zone_seed};

/// Goblins placed in a new zone (fewer if the cave has no room).
pub const GOBLINS: usize = 6;
/// Closest a goblin may start to the player, in steps.
pub const GOBLIN_MIN_DISTANCE: i32 = 12;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageLog {
    messages: VecDeque<String>,
}

impl MessageLog {
    pub fn push(&mut self, message: impl Into<String>) {
        self.messages.push_back(message.into());
        if self.messages.len() > 200 {
            self.messages.pop_front();
        }
    }

    /// Most recent message first.
    pub fn recent(&self, n: usize) -> impl Iterator<Item = &String> {
        self.messages.iter().rev().take(n)
    }
}

pub struct GameState {
    pub(crate) world: World,
    pub map: Map,
    pub log: MessageLog,
    /// One stream per subsystem (ADR 0005).
    pub rngs: Rngs,
    pub(crate) commands: VecDeque<Command>,
    pub(crate) events: EventRegistry,
    pub blueprints: Blueprints,
    pub(crate) player: Entity,
    pub seed: u64,
    /// The global clock (ADR 0006).
    pub now: Tick,
    pub timers: Timers,
    /// Set when the player dies; the scheduler stops and input is ignored.
    pub game_over: bool,
    /// Tiles the player sees now: the visible set, as one flag per tile.
    /// Recomputed once per player turn (`Command::UpdateFov`), never per
    /// actor (ADR 0007).
    pub visible: Grid<bool>,
    /// Tiles the player has ever seen. Only their terrain is remembered.
    pub seen: Grid<bool>,
    pub(crate) next_spawn_seq: u64,
}

impl GameState {
    /// The starting zone: a generated cave, the player on the floor tile
    /// nearest its centre, and `GOBLINS` goblins on free floor at least
    /// `GOBLIN_MIN_DISTANCE` away, drawn from the spawn stream.
    pub fn new(
        seed: u64,
        blueprints: Blueprints,
        cave: &CaveParams,
    ) -> Result<Self, BlueprintError> {
        let mut state = Self::bare(seed, blueprints, cave)?;
        state.populate()?;
        Ok(state)
    }

    /// Just the generated zone and the player; no other entities.
    pub fn bare(
        seed: u64,
        blueprints: Blueprints,
        cave: &CaveParams,
    ) -> Result<Self, BlueprintError> {
        let zone_seed = zone_seed(seed, ZoneCoord::START);
        let (map, map_stream) = generate_cave(zone_seed, cave);
        let mut rngs = Rngs::new(seed, zone_seed);
        rngs.map = map_stream;
        let (x, y) = start_position(&map).unwrap_or((map.width / 2, map.height / 2));
        Self::with_map(seed, rngs, map, Position { x, y }, blueprints)
    }

    fn populate(&mut self) -> Result<(), BlueprintError> {
        let Ok(from) = self
            .world
            .get::<&Position>(self.player)
            .map(|pos| (pos.x, pos.y))
        else {
            return Ok(());
        };
        for _ in 0..GOBLINS {
            let occupied: Vec<(i32, i32)> = self
                .world
                .query::<&Position>()
                .iter()
                .map(|pos| (pos.x, pos.y))
                .collect();
            let Some((x, y)) = choose_spawn_position(
                &self.map,
                &occupied,
                from,
                GOBLIN_MIN_DISTANCE,
                &mut self.rngs.spawn,
            ) else {
                break;
            };
            self.spawn_at("Goblin", Position { x, y })?;
        }
        self.log.push("Goblins prowl these caves.");
        Ok(())
    }

    fn with_map(
        seed: u64,
        rngs: Rngs,
        map: Map,
        player_at: Position,
        blueprints: Blueprints,
    ) -> Result<Self, BlueprintError> {
        let unseen = Grid::new(map.width, map.height, false);

        let mut log = MessageLog::default();
        log.push(format!("Seed: {seed}"));

        let mut state = Self {
            world: World::new(),
            map,
            log,
            rngs,
            commands: VecDeque::new(),
            events: event_registry(),
            blueprints,
            player: Entity::DANGLING,
            seed,
            now: Tick::default(),
            timers: Timers::default(),
            game_over: false,
            visible: unseen.clone(),
            seen: unseen,
            next_spawn_seq: 0,
        };
        state.player = state.spawn_at("Player", player_at)?;
        fov::update(&mut state);
        Ok(state)
    }

    /// Spawns a blueprint and registers it and its equipment with the
    /// scheduler: a `SpawnSeq` each, for tie-breaking and save order, and,
    /// for anything with a `Speed`, `Energy(0)` so it can act on the tick it
    /// appears. These spawn functions are the only way to create entities
    /// (ADR 0008); clippy rejects `World::spawn` elsewhere in `game`.
    pub(crate) fn spawn(&mut self, name: &str) -> Result<Entity, BlueprintError> {
        let id = self
            .blueprints
            .id(name)
            .ok_or_else(|| BlueprintError::UnknownBlueprint(name.to_string()))?;
        self.spawn_id(id)
    }

    pub(crate) fn spawn_id(&mut self, id: BlueprintId) -> Result<Entity, BlueprintError> {
        let entity = self.blueprints.spawn_id(&mut self.world, id)?;
        self.register(entity);
        Ok(entity)
    }

    /// Owner first, then its equipment depth-first, matching save order.
    fn register(&mut self, entity: Entity) {
        let seq = SpawnSeq(self.next_spawn_seq);
        self.next_spawn_seq += 1;
        // Inserting into an entity spawned just before cannot fail.
        if self.world.satisfies::<&Speed>(entity) {
            let _ = self.world.insert(entity, (seq, Energy(0)));
        } else {
            let _ = self.world.insert_one(entity, seq);
        }
        let carried = self
            .world
            .get::<&Attachments>(entity)
            .map(|attachments| attachments.0.clone())
            .unwrap_or_default();
        for item in carried {
            self.register(item);
        }
    }

    pub(crate) fn spawn_at(&mut self, name: &str, at: Position) -> Result<Entity, BlueprintError> {
        let entity = self.spawn(name)?;
        let _ = self.world.insert_one(entity, at);
        Ok(entity)
    }
}

/// Every event handler in the game. Handlers are code, so a loaded save
/// rebuilds this rather than persisting it.
pub(crate) fn event_registry() -> EventRegistry {
    let mut events = EventRegistry::new();
    combat::register(&mut events);
    events
}

/// A `width` × `height` view through the camera following the player
/// (`camera::camera`). Visible tiles show terrain and whatever stands there;
/// remembered tiles show dimmed terrain only; never-seen tiles and padding
/// outside the zone stay blank.
pub fn render_view(state: &GameState, width: i32, height: i32) -> Grid<Cell> {
    let camera = camera(state, width, height);
    let mut grid = Grid::new(
        camera.width,
        camera.height,
        Cell {
            glyph: ' ',
            fg: Rgb(200, 200, 200),
            bg: Rgb(0, 0, 0),
        },
    );
    let flag = |flags: &Grid<bool>, x, y| flags.get(x, y) == Some(&true);

    for screen_y in 0..camera.height {
        for screen_x in 0..camera.width {
            let (x, y) = camera.screen_to_world((screen_x, screen_y));
            let visible = flag(&state.visible, x, y);
            if !visible && !flag(&state.seen, x, y) {
                continue;
            }
            let tile = state.map.get(x, y).copied().unwrap_or(Tile::Wall);
            let (glyph, fg) = match tile {
                Tile::Floor => ('.', Rgb(90, 90, 90)),
                Tile::Wall => ('#', Rgb(140, 140, 140)),
            };
            if let Some(cell) = grid.get_mut(screen_x, screen_y) {
                cell.glyph = glyph;
                cell.fg = if visible { fg } else { dim(fg) };
            }
        }
    }

    for (pos, appearance) in state.world.query::<(&Position, &Appearance)>().iter() {
        if !flag(&state.visible, pos.x, pos.y) {
            continue;
        }
        let (screen_x, screen_y) = camera.world_to_screen((pos.x, pos.y));
        if let Some(cell) = grid.get_mut(screen_x, screen_y) {
            cell.glyph = appearance.glyph;
            cell.fg = appearance.fg;
        }
    }

    grid
}

/// Remembered terrain: half brightness.
fn dim(Rgb(r, g, b): Rgb) -> Rgb {
    Rgb(r / 2, g / 2, b / 2)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::blueprint::{Blueprint, ComponentDef};
    use crate::combat::roll_melee;
    use crate::command::apply_commands;
    use crate::entity::{Armor, Attachments, Bleeding, Duration, Health, Melee, Name};
    use crate::input::{Dir, PlayerAction};
    use crate::map::tests::room;
    use crate::zone::tests::test_cave;
    use rand::Rng;

    fn blueprint(components: Vec<ComponentDef>, equipment: &[&str]) -> Blueprint {
        Blueprint {
            inherits: None,
            components,
            equipment: Some(equipment.iter().map(|s| s.to_string()).collect()),
        }
    }

    fn look(glyph: char) -> ComponentDef {
        ComponentDef::Appearance(Appearance {
            glyph,
            fg: Rgb(255, 255, 255),
            bg: Rgb(0, 0, 0),
        })
    }

    /// Player: 20 HP, 2-4 melee. Goblin: 8 HP wearing 1-AV armor, hostile.
    /// Both normal speed. Bleeding: 1 damage every 100 ticks for 500 ticks.
    pub(crate) fn test_blueprints() -> Blueprints {
        let name = |n: &str| ComponentDef::Name(Name(n.to_string()));
        Blueprints::resolve(BTreeMap::from([
            (
                "Player".to_string(),
                blueprint(
                    vec![
                        name("player"),
                        ComponentDef::Player,
                        look('@'),
                        ComponentDef::BlocksMovement,
                        ComponentDef::Health(Health {
                            current: 20,
                            max: 20,
                        }),
                        ComponentDef::Melee(Melee { min: 2, max: 4 }),
                        ComponentDef::Speed(Speed(100)),
                    ],
                    &[],
                ),
            ),
            (
                "Rubble".to_string(),
                blueprint(
                    vec![name("rubble"), look('%'), ComponentDef::BlocksMovement],
                    &[],
                ),
            ),
            (
                "Goblin".to_string(),
                blueprint(
                    vec![
                        name("goblin"),
                        look('g'),
                        ComponentDef::BlocksMovement,
                        ComponentDef::Health(Health { current: 8, max: 8 }),
                        ComponentDef::Melee(Melee { min: 1, max: 3 }),
                        ComponentDef::Speed(Speed(100)),
                        ComponentDef::Hostile,
                    ],
                    &["LeatherArmor"],
                ),
            ),
            (
                "Bleeding".to_string(),
                blueprint(
                    vec![
                        name("bleeding"),
                        ComponentDef::Bleeding(Bleeding {
                            damage: 1,
                            interval: 100,
                        }),
                        ComponentDef::Duration(Duration(500)),
                    ],
                    &[],
                ),
            ),
            (
                "LeatherArmor".to_string(),
                blueprint(
                    vec![name("leather armor"), ComponentDef::Armor(Armor { av: 1 })],
                    &[],
                ),
            ),
            ("Blank".to_string(), blueprint(vec![], &[])),
        ]))
        .expect("test blueprints resolve")
    }

    /// The hand-built `room` with the player at (1, 1) and nothing else, so
    /// tests control every other entity and generator tuning can't move
    /// walls under them.
    pub(crate) fn bare_state() -> GameState {
        bare_state_seeded(1)
    }

    /// `bare_state` with `seed`'s RNG streams.
    pub(crate) fn bare_state_seeded(seed: u64) -> GameState {
        let rngs = Rngs::new(seed, seed);
        GameState::with_map(
            seed,
            rngs,
            room(),
            Position { x: 1, y: 1 },
            test_blueprints(),
        )
        .unwrap()
    }

    /// An entity with exactly `components` (plus its `SpawnSeq`), spawned
    /// through `GameState::spawn` from the empty `Blank` blueprint. A
    /// `SpawnSeq` in `components` replaces the assigned one.
    pub(crate) fn spawn_with(
        state: &mut GameState,
        components: impl hecs::DynamicBundle,
    ) -> Entity {
        let entity = state.spawn("Blank").unwrap();
        state.world.insert(entity, components).unwrap();
        entity
    }

    fn player_position(state: &GameState) -> Position {
        *state.world.get::<&Position>(state.player).unwrap()
    }

    fn goblins(state: &GameState) -> Vec<(Position, Vec<Entity>)> {
        state
            .world
            .query::<(&Name, &Position, &Attachments)>()
            .iter()
            .filter(|(name, _, _)| name.0 == "goblin")
            .map(|(_, pos, attachments)| (*pos, attachments.0.clone()))
            .collect()
    }

    #[test]
    fn new_spawns_armored_goblins_on_floor_away_from_the_player() {
        for seed in 0..5 {
            let state = GameState::new(seed, test_blueprints(), &test_cave()).unwrap();
            let player = player_position(&state);
            let goblins = goblins(&state);
            assert_eq!(goblins.len(), GOBLINS, "seed {seed}");
            for (at, gear) in &goblins {
                assert_eq!(state.map.get(at.x, at.y), Some(&Tile::Floor));
                let distance = (at.x - player.x).abs().max((at.y - player.y).abs());
                assert!(distance >= GOBLIN_MIN_DISTANCE, "seed {seed}: {at:?}");
                assert!(state.world.satisfies::<&Armor>(gear[0]));
            }
        }
    }

    #[test]
    fn the_player_starts_on_the_floor_tile_nearest_the_centre() {
        let state = GameState::bare(3, test_blueprints(), &test_cave()).unwrap();
        let start = player_position(&state);
        let (cx, cy) = (state.map.width / 2, state.map.height / 2);
        let dist = |x: i32, y: i32| (x - cx).pow(2) + (y - cy).pow(2);
        let nearest = crate::map::floor_candidates(&state.map, &[])
            .into_iter()
            .map(|(x, y)| dist(x, y))
            .min();
        assert_eq!(Some(dist(start.x, start.y)), nearest);
    }

    fn positions(state: &GameState) -> Vec<(Position, Option<String>)> {
        let mut all: Vec<_> = state
            .world
            .query::<(&Position, Option<&Name>)>()
            .iter()
            .map(|(pos, name)| (*pos, name.map(|n| n.0.clone())))
            .collect();
        all.sort_by_key(|(pos, name)| (pos.x, pos.y, name.clone()));
        all
    }

    fn combat_rolls(state: &mut GameState) -> Vec<i32> {
        (0..20)
            .map(|_| roll_melee(&state.world, &mut state.rngs.combat, state.player).unwrap())
            .collect()
    }

    #[test]
    fn same_seed_yields_identical_map_spawns_and_combat_rolls() {
        let mut a = GameState::new(9, test_blueprints(), &test_cave()).unwrap();
        let mut b = GameState::new(9, test_blueprints(), &test_cave()).unwrap();
        assert_eq!(a.map, b.map);
        assert_eq!(positions(&a), positions(&b));
        assert_eq!(combat_rolls(&mut a), combat_rolls(&mut b));
    }

    #[test]
    fn an_extra_spawn_draw_leaves_map_and_combat_streams_alone() {
        let mut fresh = GameState::new(9, test_blueprints(), &test_cave()).unwrap();

        let mut perturbed = GameState::bare(9, test_blueprints(), &test_cave()).unwrap();
        perturbed.rngs.spawn.next_u64();
        perturbed.populate().unwrap();

        assert_eq!(perturbed.map, fresh.map);
        assert_eq!(perturbed.rngs.map, fresh.rngs.map);
        assert_ne!(perturbed.rngs.spawn, fresh.rngs.spawn);
        assert_eq!(combat_rolls(&mut perturbed), combat_rolls(&mut fresh));
    }

    #[test]
    fn spawns_avoid_walls_the_player_and_each_other() {
        for seed in 0..10 {
            let state = GameState::new(seed, test_blueprints(), &test_cave()).unwrap();
            let tiles: Vec<Position> = positions(&state).into_iter().map(|(p, _)| p).collect();
            for (i, pos) in tiles.iter().enumerate() {
                assert_eq!(
                    state.map.get(pos.x, pos.y),
                    Some(&Tile::Floor),
                    "seed {seed}"
                );
                assert!(
                    !tiles[i + 1..].contains(pos),
                    "seed {seed}: overlap at {pos:?}"
                );
            }
        }
    }

    #[test]
    fn missing_player_blueprint_is_an_error() {
        assert!(GameState::bare(1, Blueprints::default(), &test_cave()).is_err());
    }

    #[test]
    fn move_onto_open_floor_succeeds() {
        let mut state = bare_state();
        state
            .commands
            .push_back(Command::TryMove(state.player, Dir::E));
        apply_commands(&mut state);
        assert_eq!(player_position(&state), Position { x: 2, y: 1 });
    }

    #[test]
    fn move_into_wall_is_blocked_and_logs_a_message() {
        let mut state = bare_state();
        // (1,1) -> north is (1,0), a wall in `room`.
        state
            .commands
            .push_back(Command::TryMove(state.player, Dir::N));
        apply_commands(&mut state);
        assert_eq!(player_position(&state), Position { x: 1, y: 1 });
        assert_eq!(
            state.log.recent(1).next().map(String::as_str),
            Some("You bump into a wall.")
        );
    }

    #[test]
    fn move_into_blocking_entity_is_blocked_and_logs_a_message() {
        let mut state = bare_state();
        state.spawn_at("Rubble", Position { x: 2, y: 1 }).unwrap();
        state
            .commands
            .push_back(Command::TryMove(state.player, Dir::E));
        apply_commands(&mut state);
        assert_eq!(player_position(&state), Position { x: 1, y: 1 });
        assert_eq!(
            state.log.recent(1).next().map(String::as_str),
            Some("Something blocks your way.")
        );
    }

    #[test]
    fn remembered_terrain_renders_dimmed_without_creatures() {
        let mut state = bare_state();
        for _ in 0..10 {
            crate::time::take_turn(&mut state, PlayerAction::Move(Dir::E));
        }
        assert_eq!(player_position(&state), Position { x: 11, y: 1 });
        // (2, 1) was seen from the start and is now 9 tiles away.
        assert_eq!(state.visible.get(2, 1), Some(&false));
        assert_eq!(state.seen.get(2, 1), Some(&true));
        state.spawn_at("Goblin", Position { x: 2, y: 1 }).unwrap();

        // The whole room fits, so screen and world coordinates agree.
        let view = render_view(&state, 40, 20);
        let remembered = view.get(2, 1).unwrap();
        assert_eq!((remembered.glyph, remembered.fg), ('.', Rgb(45, 45, 45)));
        let in_view = view.get(10, 1).unwrap();
        assert_eq!((in_view.glyph, in_view.fg), ('.', Rgb(90, 90, 90)));
        let never_seen = view.get(38, 18).unwrap();
        assert_eq!(never_seen.glyph, ' ');
    }

    #[test]
    fn the_view_is_the_viewport_size_and_centred_on_the_player() {
        let state = GameState::bare(3, test_blueprints(), &test_cave()).unwrap();
        let view = render_view(&state, 41, 21);
        assert_eq!((view.width, view.height), (41, 21));
        assert_eq!(view.get(20, 10).map(|cell| cell.glyph), Some('@'));
    }

    #[test]
    fn diagonal_move_succeeds() {
        let mut state = bare_state();
        state
            .commands
            .push_back(Command::TryMove(state.player, Dir::SE));
        apply_commands(&mut state);
        assert_eq!(player_position(&state), Position { x: 2, y: 2 });
    }
}
