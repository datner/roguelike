//! `GameState`, the message log, and `render_view`, which turns what the player knows into a `Grid<Cell>`.

use std::collections::VecDeque;

use hecs::{Entity, World};

use crate::blueprint::{BlueprintError, Blueprints};
use crate::cell::{Cell, Rgb};
use crate::combat;
use crate::command::Command;
use crate::entity::{Appearance, Energy, Position, SpawnSeq, Speed};
use crate::event::EventRegistry;
use crate::fov;
use crate::map::{Grid, Map, Tile, choose_spawn_position, generate_map};
use crate::rng::Rngs;
use crate::time::{Tick, Timers};

#[derive(Debug, Clone, Default, PartialEq)]
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
    pub world: World,
    pub map: Map,
    pub log: MessageLog,
    /// One stream per subsystem (ADR 0005).
    pub rngs: Rngs,
    pub commands: VecDeque<Command>,
    pub events: EventRegistry,
    pub blueprints: Blueprints,
    pub player: Entity,
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
    next_spawn_seq: u64,
}

impl GameState {
    /// The starting room: the player at (1, 1), plus a `Rubble` and a
    /// `Goblin`, each on a free floor tile drawn from the spawn stream.
    pub fn new(seed: u64, blueprints: Blueprints) -> Result<Self, BlueprintError> {
        let mut state = Self::bare(seed, blueprints)?;
        state.populate()?;
        Ok(state)
    }

    /// Just the map and the player (spawned from the `Player` blueprint at
    /// (1, 1)); no other entities.
    pub fn bare(seed: u64, blueprints: Blueprints) -> Result<Self, BlueprintError> {
        Self::bare_with_rngs(seed, Rngs::new(seed), blueprints)
    }

    fn populate(&mut self) -> Result<(), BlueprintError> {
        for name in ["Rubble", "Goblin"] {
            let occupied: Vec<(i32, i32)> = self
                .world
                .query::<&Position>()
                .iter()
                .map(|pos| (pos.x, pos.y))
                .collect();
            let (x, y) = choose_spawn_position(&self.map, &occupied, &mut self.rngs.spawn)
                .expect("the starting room has free floor");
            self.spawn_at(name, Position { x, y })?;
        }
        self.log.push("A goblin lurks in the room.");
        Ok(())
    }

    fn bare_with_rngs(
        seed: u64,
        mut rngs: Rngs,
        blueprints: Blueprints,
    ) -> Result<Self, BlueprintError> {
        let map = generate_map(&mut rngs.map);
        let unseen = Grid::new(map.width, map.height, false);

        let mut events = EventRegistry::new();
        combat::register(&mut events);

        let mut log = MessageLog::default();
        log.push(format!("Seed: {seed}"));

        let mut state = Self {
            world: World::new(),
            map,
            log,
            rngs,
            commands: VecDeque::new(),
            events,
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
        state.player = state.spawn_at("Player", Position { x: 1, y: 1 })?;
        fov::update(&mut state);
        Ok(state)
    }

    /// Spawns a blueprint and registers it with the scheduler: a `SpawnSeq`
    /// for tie-breaking, and, if it has a `Speed`, `Energy(0)` so it can act
    /// on the tick it appears. Use this rather than `Blueprints::spawn`
    /// during play.
    pub fn spawn(&mut self, name: &str) -> Result<Entity, BlueprintError> {
        let entity = self.blueprints.spawn(&mut self.world, name)?;
        let seq = SpawnSeq(self.next_spawn_seq);
        self.next_spawn_seq += 1;
        // Inserting into an entity spawned just above cannot fail.
        if self.world.satisfies::<&Speed>(entity) {
            let _ = self.world.insert(entity, (seq, Energy(0)));
        } else {
            let _ = self.world.insert_one(entity, seq);
        }
        Ok(entity)
    }

    pub fn spawn_at(&mut self, name: &str, at: Position) -> Result<Entity, BlueprintError> {
        let entity = self.spawn(name)?;
        let _ = self.world.insert_one(entity, at);
        Ok(entity)
    }
}

/// Visible tiles show terrain and whatever stands there; remembered tiles
/// show dimmed terrain only; never-seen tiles stay blank.
pub fn render_view(state: &GameState) -> Grid<Cell> {
    let mut grid = Grid::new(
        state.map.width,
        state.map.height,
        Cell {
            glyph: ' ',
            fg: Rgb(200, 200, 200),
            bg: Rgb(0, 0, 0),
        },
    );
    let flag = |flags: &Grid<bool>, x, y| flags.get(x, y) == Some(&true);

    for y in 0..state.map.height {
        for x in 0..state.map.width {
            let visible = flag(&state.visible, x, y);
            if !visible && !flag(&state.seen, x, y) {
                continue;
            }
            let tile = state.map.get(x, y).copied().unwrap_or(Tile::Wall);
            let (glyph, fg) = match tile {
                Tile::Floor => ('.', Rgb(90, 90, 90)),
                Tile::Wall => ('#', Rgb(140, 140, 140)),
            };
            if let Some(cell) = grid.get_mut(x, y) {
                cell.glyph = glyph;
                cell.fg = if visible { fg } else { dim(fg) };
            }
        }
    }

    for (pos, appearance) in state.world.query::<(&Position, &Appearance)>().iter() {
        if !flag(&state.visible, pos.x, pos.y) {
            continue;
        }
        if let Some(cell) = grid.get_mut(pos.x, pos.y) {
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
    use crate::entity::{
        Armor, Attachments, Bleeding, BlocksMovement, Duration, Health, Melee, Name,
    };
    use crate::input::Dir;
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
        ]))
        .expect("test blueprints resolve")
    }

    /// Map and player only, so tests control every other entity.
    pub(crate) fn bare_state() -> GameState {
        GameState::bare(1, test_blueprints()).unwrap()
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
    fn new_spawns_an_armored_goblin_away_from_the_rubble() {
        let state = GameState::new(1, test_blueprints()).unwrap();
        let goblins = goblins(&state);
        assert_eq!(goblins.len(), 1);
        let (goblin_at, gear) = &goblins[0];
        assert!(state.world.satisfies::<&Armor>(gear[0]));

        let rubble_at = state
            .world
            .query::<(&Position, &BlocksMovement)>()
            .without::<&Health>()
            .iter()
            .map(|(pos, _)| *pos)
            .next()
            .unwrap();
        assert_ne!(*goblin_at, rubble_at);
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
        let mut a = GameState::new(9, test_blueprints()).unwrap();
        let mut b = GameState::new(9, test_blueprints()).unwrap();
        assert_eq!(a.map, b.map);
        assert_eq!(positions(&a), positions(&b));
        assert_eq!(combat_rolls(&mut a), combat_rolls(&mut b));
    }

    #[test]
    fn an_extra_spawn_draw_leaves_map_and_combat_streams_alone() {
        let mut fresh = GameState::new(9, test_blueprints()).unwrap();

        let mut rngs = Rngs::new(9);
        rngs.spawn.next_u64();
        let mut perturbed = GameState::bare_with_rngs(9, rngs, test_blueprints()).unwrap();
        perturbed.populate().unwrap();

        assert_eq!(perturbed.map, fresh.map);
        assert_eq!(perturbed.rngs.map, fresh.rngs.map);
        assert_ne!(perturbed.rngs.spawn, fresh.rngs.spawn);
        assert_eq!(combat_rolls(&mut perturbed), combat_rolls(&mut fresh));
    }

    #[test]
    fn spawns_avoid_walls_the_player_and_each_other() {
        for seed in 0..200 {
            let state = GameState::new(seed, test_blueprints()).unwrap();
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
        assert!(GameState::bare(1, Blueprints::default()).is_err());
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
        // (1,1) -> north is (1,0), which generate_map always makes a wall.
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
        state
            .blueprints
            .spawn_at(&mut state.world, "Rubble", Position { x: 2, y: 1 })
            .unwrap();
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
        let player = state.player;
        for _ in 0..10 {
            crate::time::take_turn(&mut state, Command::TryMove(player, Dir::E));
        }
        assert_eq!(player_position(&state), Position { x: 11, y: 1 });
        // (2, 1) was seen from the start and is now 9 tiles away.
        assert_eq!(state.visible.get(2, 1), Some(&false));
        assert_eq!(state.seen.get(2, 1), Some(&true));
        state.spawn_at("Goblin", Position { x: 2, y: 1 }).unwrap();

        let view = render_view(&state);
        let remembered = view.get(2, 1).unwrap();
        assert_eq!((remembered.glyph, remembered.fg), ('.', Rgb(45, 45, 45)));
        let in_view = view.get(10, 1).unwrap();
        assert_eq!((in_view.glyph, in_view.fg), ('.', Rgb(90, 90, 90)));
        let never_seen = view.get(38, 18).unwrap();
        assert_eq!(never_seen.glyph, ' ');
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
