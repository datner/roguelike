use std::collections::VecDeque;

use hecs::{Entity, World};
use rand_pcg::Pcg64;

use crate::blueprint::EntityBlueprint;
use crate::cell::{Cell, Rgb};
use crate::command::Command;
use crate::entity::{Appearance, BlocksMovement, Position};
use crate::map::{choose_obstacle_position, generate_map, Grid, Tile};

#[derive(Debug, Default)]
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
    pub map: Grid<Tile>,
    pub log: MessageLog,
    pub rng: Pcg64,
    pub commands: VecDeque<Command>,
    pub player: Entity,
    pub seed: u64,
}

impl GameState {
    pub fn new(seed: u64, obstacle_blueprint: EntityBlueprint) -> Self {
        let (map, mut rng) = generate_map(seed);
        let mut world = World::new();

        let player = world.spawn((
            Position { x: 1, y: 1 },
            Appearance {
                glyph: '@',
                fg: Rgb(255, 255, 255),
                bg: Rgb(0, 0, 0),
            },
        ));

        let (ox, oy) = choose_obstacle_position(&map, &mut rng);
        let obstacle_appearance = Appearance {
            glyph: obstacle_blueprint.glyph,
            fg: obstacle_blueprint.fg,
            bg: obstacle_blueprint.bg,
        };
        if obstacle_blueprint.blocks_movement {
            world.spawn((
                Position { x: ox, y: oy },
                obstacle_appearance,
                BlocksMovement,
            ));
        } else {
            world.spawn((Position { x: ox, y: oy }, obstacle_appearance));
        }

        let mut log = MessageLog::default();
        log.push(format!("Seed: {seed}"));
        log.push(format!("A {} blocks part of the room.", obstacle_blueprint.name));

        Self {
            world,
            map,
            log,
            rng,
            commands: VecDeque::new(),
            player,
            seed,
        }
    }
}

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

    for y in 0..state.map.height {
        for x in 0..state.map.width {
            let tile = state.map.get(x, y).copied().unwrap_or(Tile::Wall);
            let (glyph, fg) = match tile {
                Tile::Floor => ('.', Rgb(90, 90, 90)),
                Tile::Wall => ('#', Rgb(140, 140, 140)),
            };
            if let Some(cell) = grid.get_mut(x, y) {
                cell.glyph = glyph;
                cell.fg = fg;
            }
        }
    }

    for (pos, appearance) in state.world.query::<(&Position, &Appearance)>().iter() {
        if let Some(cell) = grid.get_mut(pos.x, pos.y) {
            cell.glyph = appearance.glyph;
            cell.fg = appearance.fg;
        }
    }

    grid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::apply_commands;
    use crate::input::Dir;

    fn no_obstacle_blueprint() -> EntityBlueprint {
        // blocks_movement: false so tests can reason about wall collision
        // in isolation, without the obstacle entity interfering.
        EntityBlueprint {
            name: "test-prop".to_string(),
            glyph: '?',
            fg: Rgb(1, 2, 3),
            bg: Rgb(0, 0, 0),
            blocks_movement: false,
        }
    }

    fn blocking_obstacle_blueprint() -> EntityBlueprint {
        EntityBlueprint {
            name: "rubble".to_string(),
            glyph: '%',
            fg: Rgb(180, 140, 90),
            bg: Rgb(0, 0, 0),
            blocks_movement: true,
        }
    }

    fn player_position(state: &GameState) -> Position {
        *state.world.get::<&Position>(state.player).unwrap()
    }

    #[test]
    fn move_onto_open_floor_succeeds() {
        let mut state = GameState::new(1, no_obstacle_blueprint());
        state.commands.push_back(Command::TryMove(state.player, Dir::E));
        apply_commands(&mut state);
        assert_eq!(player_position(&state), Position { x: 2, y: 1 });
    }

    #[test]
    fn move_into_wall_is_blocked_and_logs_a_message() {
        let mut state = GameState::new(1, no_obstacle_blueprint());
        // (1,1) -> north is (1,0), which generate_map always makes a wall.
        state.commands.push_back(Command::TryMove(state.player, Dir::N));
        apply_commands(&mut state);
        assert_eq!(player_position(&state), Position { x: 1, y: 1 });
        assert_eq!(
            state.log.recent(1).next().map(String::as_str),
            Some("You bump into a wall.")
        );
    }

    #[test]
    fn move_into_blocking_entity_is_blocked_and_logs_a_message() {
        let mut state = GameState::new(3, blocking_obstacle_blueprint());
        let (ox, oy) = state
            .world
            .query::<(&Position, &BlocksMovement)>()
            .iter()
            .map(|(pos, _)| (pos.x, pos.y))
            .next()
            .expect("blocking obstacle should have spawned");
        // Walk the player next to the obstacle, then step onto it.
        state.world.get::<&mut Position>(state.player).unwrap().x = ox - 1;
        state.world.get::<&mut Position>(state.player).unwrap().y = oy;
        state.commands.push_back(Command::TryMove(state.player, Dir::E));
        apply_commands(&mut state);
        assert_eq!(player_position(&state), Position { x: ox - 1, y: oy });
        assert_eq!(
            state.log.recent(1).next().map(String::as_str),
            Some("Something blocks your way.")
        );
    }

    #[test]
    fn diagonal_move_succeeds() {
        let mut state = GameState::new(1, no_obstacle_blueprint());
        state.commands.push_back(Command::TryMove(state.player, Dir::SE));
        apply_commands(&mut state);
        assert_eq!(player_position(&state), Position { x: 2, y: 2 });
    }
}
