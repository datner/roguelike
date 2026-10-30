//! `GameState`, the message log, and `render_view`, which turns what the player knows into a `Grid<Cell>`.

use std::collections::VecDeque;

use hecs::{Entity, World};
use rand::RngExt;
use serde::{Deserialize, Serialize};

use crate::blueprint::{BlueprintError, Blueprints};
use crate::camera::camera;
use crate::cell::{Cell, Rgb};
use crate::combat;
use crate::command::Command;
use crate::entity::{Appearance, Energy, Item, Position, Quiver, SpawnSeq, Speed};
use crate::event::EventRegistry;
use crate::explore::ExploreItems;
use crate::fov;
use crate::holding;
use crate::map::{Grid, Map, Tile, choose_spawn_position};
use crate::rng::Rngs;
use crate::spawn::{Held, Source, Spawn};
use crate::spell;
use crate::time::{Tick, Timers};

/// Closest a creature may start to the player, in steps.
pub const CREATURE_MIN_DISTANCE: i32 = 12;

/// Arrows the player starts with in their quiver.
pub const STARTING_ARROWS: usize = 12;
/// Arrows in a creature's quiver when it is placed on a floor, drawn.
const CREATURE_ARROWS: std::ops::RangeInclusive<usize> = 3..=8;
/// Closest an item may start to the player, in steps, so finding one means
/// leaving the starting spot.
pub const ITEM_MIN_DISTANCE: i32 = 4;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageLog {
    messages: VecDeque<String>,
    /// Why each of the newest messages happened, where it says
    /// (docs/plans/legibility.md): aligned with the end of `messages`, and
    /// never saved, so a loaded log has none.
    #[serde(skip)]
    whys: VecDeque<Option<String>>,
}

/// Logs are the same when their messages are: the whys are for now.
impl PartialEq for MessageLog {
    fn eq(&self, other: &Self) -> bool {
        self.messages == other.messages
    }
}

impl MessageLog {
    pub fn push(&mut self, message: impl Into<String>) {
        self.push_why(message, None);
    }

    /// `push`, with why it happened.
    pub fn push_with(&mut self, message: impl Into<String>, why: impl Into<String>) {
        self.push_why(message, Some(why.into()));
    }

    fn push_why(&mut self, message: impl Into<String>, why: Option<String>) {
        self.messages.push_back(message.into());
        self.whys.push_back(why);
        if self.messages.len() > 200 {
            if self.whys.len() >= self.messages.len() {
                self.whys.pop_front();
            }
            self.messages.pop_front();
        }
    }

    /// Most recent message first.
    pub fn recent(&self, n: usize) -> impl Iterator<Item = &String> {
        self.messages.iter().rev().take(n)
    }

    /// Most recent message first, each with its why, if it has one.
    pub fn recent_with_why(&self, n: usize) -> impl Iterator<Item = (&String, Option<&str>)> {
        let whys = self
            .whys
            .iter()
            .rev()
            .map(Option::as_deref)
            .chain(std::iter::repeat(None));
        self.messages.iter().rev().zip(whys).take(n)
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
    /// What last hurt the player, as it reads ("the goblin's blow"), and by
    /// how much: what a death names (docs/plans/legibility.md). Not saved.
    pub(crate) last_hurt: Option<(String, i32)>,
    /// How the player died, once they have. Not saved: a dead character's
    /// save is gone.
    pub(crate) death: Option<crate::view::DeathView>,
    /// Tiles the player sees now: the visible set, as one flag per tile.
    /// Recomputed once per player turn (`Command::UpdateFov`), never per
    /// actor (ADR 0007).
    pub visible: Grid<bool>,
    /// Tiles the player has ever seen; their terrain is remembered.
    pub seen: Grid<bool>,
    /// The item last seen on each tile, drawn dimmed while out of view.
    /// Refreshed only for visible tiles, so it goes stale like the
    /// player's memory would.
    pub remembered_items: Grid<Option<Appearance>>,
    pub(crate) next_spawn_seq: u64,
    /// What auto-explore does on reaching a floor item. A player setting,
    /// but it decides actions, so it is game state and saved.
    pub explore_items: ExploreItems,
    /// Which floor of which dive the player is on, or camp (`dive`).
    pub place: crate::dive::Place,
    /// Dives started, so each gets its own floors.
    pub dives: u32,
    /// The phase the next dive will be in, chosen at camp (`phase`).
    pub phase: String,
    /// Effects since the frontend last took them, for animation only:
    /// never read by the simulation, never saved (ADR 0022).
    pub(crate) fx: Vec<crate::fx::Fx>,
}

impl GameState {
    /// The first floor of the first dungeon (`dive::build`), filled from
    /// the dungeon's spawn table by the spawn stream.
    pub fn new(seed: u64, blueprints: Blueprints) -> Result<Self, BlueprintError> {
        Self::new_as(seed, blueprints, None)
    }

    /// `new`, the player starting as `origin` (a key in `data/origins.ron`;
    /// `None` for the first). The world is the same for every origin.
    pub fn new_as(
        seed: u64,
        blueprints: Blueprints,
        origin: Option<&str>,
    ) -> Result<Self, BlueprintError> {
        Self::new_in_lineage(seed, blueprints, origin, &[])
    }

    /// `new_as`, finding at camp the ruins of the lineage's past lives
    /// (`records`, newest first; docs/plans/legacy.md). Records are an input
    /// like the seed: the same ones give the same game.
    pub fn new_in_lineage(
        seed: u64,
        blueprints: Blueprints,
        origin: Option<&str>,
        records: &[crate::legacy::Record],
    ) -> Result<Self, BlueprintError> {
        let mut state = Self::bare_as(seed, blueprints, origin)?;
        state.populate()?;
        crate::legacy::build_ruins(&mut state, records)?;
        Ok(state)
    }

    /// Just the starting place and the player: camp if the content has one,
    /// else the first dungeon's first floor, on its stairs up. No other
    /// entities.
    pub fn bare(seed: u64, blueprints: Blueprints) -> Result<Self, BlueprintError> {
        Self::bare_as(seed, blueprints, None)
    }

    /// `bare`, the player starting as `origin`.
    pub fn bare_as(
        seed: u64,
        blueprints: Blueprints,
        origin: Option<&str>,
    ) -> Result<Self, BlueprintError> {
        if let Some(camp) = blueprints.camp() {
            let start = crate::dive::start(seed, camp);
            let mut rngs = Rngs::new(seed, 0);
            rngs.map = start.map_stream;
            rngs.spawn = start.spawn_stream;
            let (x, y) = start.arrival;
            let mut state =
                Self::with_map(seed, rngs, start.map, Position { x, y }, blueprints, origin)?;
            state.place = crate::dive::Place::Camp;
            return Ok(state);
        }
        let dungeon = blueprints
            .first_dungeon()
            .ok_or(BlueprintError::NoDungeon)?
            .to_string();
        let found = blueprints
            .dungeon(&dungeon)
            .ok_or(BlueprintError::NoDungeon)?
            .clone();
        let phase = blueprints
            .phases()
            .first()
            .map(|phase| phase.key.clone())
            .unwrap_or_default();
        let floor = crate::dive::build(seed, &found, &dungeon, &phase, 1, 1);
        let mut rngs = Rngs::new(seed, 0);
        rngs.map = floor.map_stream;
        rngs.spawn = floor.spawn_stream;
        let (x, y) = floor.arrival;
        let mut state =
            Self::with_map(seed, rngs, floor.map, Position { x, y }, blueprints, origin)?;
        state.place = crate::dive::Place::Floor {
            dungeon,
            phase,
            dive: 1,
            floor: 1,
        };
        Ok(state)
    }

    /// Arrows in the player's quiver at the start: a bow is no use without,
    /// and no RNG is drawn, so seeded worlds are unchanged by it.
    pub(crate) fn stock_quivers(&mut self) -> Result<(), BlueprintError> {
        self.stock_quivers_of(self.player, STARTING_ARROWS)
    }

    fn quivers_of(&self, holder: Entity) -> Vec<Entity> {
        crate::equipment::equipped(&self.world, holder)
            .into_iter()
            .map(|(item, _)| item)
            .filter(|&item| self.world.satisfies::<&Quiver>(item))
            .collect()
    }

    /// `count` arrows in each quiver `holder` wears.
    fn stock_quivers_of(&mut self, holder: Entity, count: usize) -> Result<(), BlueprintError> {
        for quiver in self.quivers_of(holder) {
            for _ in 0..count {
                self.spawn(Spawn::blueprint("Arrow").carried_by(quiver))?;
            }
        }
        Ok(())
    }

    pub(crate) fn populate(&mut self) -> Result<(), BlueprintError> {
        let Ok(from) = self
            .world
            .get::<&Position>(self.player)
            .map(|pos| (pos.x, pos.y))
        else {
            return Ok(());
        };
        let crate::dive::Place::Floor {
            dungeon,
            phase,
            floor,
            ..
        } = &self.place
        else {
            if let Some(line) = self.blueprints.camp().map(|camp| camp.arrival_line.clone()) {
                self.log.push(line);
            }
            return Ok(());
        };
        let (floor, phase) = (*floor, phase.clone());
        let Some(dungeon) = self.blueprints.dungeon(dungeon).cloned() else {
            return Ok(());
        };
        for inhabitant in dungeon.creatures.iter().filter(|i| i.in_phase(&phase)) {
            self.place(&inhabitant.blueprint, inhabitant.on_floor(floor), from)?;
        }
        // After the creatures, so adding items leaves their placement alone;
        // named finds after the rest, likewise.
        let here: Vec<_> = dungeon
            .items
            .iter()
            .filter(|scatter| scatter.in_phase(&phase))
            .collect();
        let any = here
            .iter()
            .filter(|scatter| scatter.blueprint.is_none())
            .map(|scatter| scatter.on_floor(floor))
            .sum();
        self.scatter_items(self.blueprints.items(), any, from)?;
        for scatter in here {
            if let Some(id) = scatter
                .blueprint
                .as_ref()
                .and_then(|b| self.blueprints.id(b))
            {
                self.scatter_items(vec![id], scatter.on_floor(floor), from)?;
            }
        }
        self.log.push(dungeon.arrival);
        Ok(())
    }

    /// Up to `count` of `blueprint`, each on a free tile at least
    /// `CREATURE_MIN_DISTANCE` from `from`.
    fn place(
        &mut self,
        blueprint: &str,
        count: usize,
        from: (i32, i32),
    ) -> Result<(), BlueprintError> {
        for _ in 0..count {
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
                CREATURE_MIN_DISTANCE,
                &mut self.rngs.spawn,
            ) else {
                break;
            };
            let creature = self.spawn(Spawn::blueprint(blueprint).at(x, y))?;
            // An archer's quiver comes with a few arrows, drawn after its
            // placement, so creatures placed before it stay put.
            if !self.quivers_of(creature).is_empty() {
                let arrows = self.rngs.spawn.random_range(CREATURE_ARROWS);
                self.stock_quivers_of(creature, arrows)?;
            }
        }
        Ok(())
    }

    /// `count` items, each picked from `pool`, away from `from`.
    fn scatter_items(
        &mut self,
        pool: Vec<crate::blueprint::BlueprintId>,
        count: usize,
        from: (i32, i32),
    ) -> Result<(), BlueprintError> {
        if pool.is_empty() {
            return Ok(());
        }
        for _ in 0..count {
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
                ITEM_MIN_DISTANCE,
                &mut self.rngs.spawn,
            ) else {
                break;
            };
            let id = pool[self.rngs.spawn.random_range(0..pool.len())];
            self.spawn(Spawn::id(id).at(x, y))?;
        }
        Ok(())
    }

    pub(crate) fn with_map(
        seed: u64,
        rngs: Rngs,
        map: Map,
        player_at: Position,
        blueprints: Blueprints,
        origin: Option<&str>,
    ) -> Result<Self, BlueprintError> {
        let unseen = Grid::new(map.width, map.height, false);
        let no_items = Grid::new(map.width, map.height, None);
        // The first dungeon until the caller says where this is.
        let first_phase = blueprints
            .phases()
            .first()
            .map(|phase| phase.key.clone())
            .unwrap_or_default();
        let place = crate::dive::Place::Floor {
            dungeon: blueprints.first_dungeon().unwrap_or_default().to_string(),
            phase: first_phase.clone(),
            dive: 1,
            floor: 1,
        };

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
            last_hurt: None,
            death: None,
            visible: unseen.clone(),
            seen: unseen,
            remembered_items: no_items,
            next_spawn_seq: 0,
            explore_items: ExploreItems::default(),
            place,
            dives: 0,
            phase: first_phase,
            fx: Vec::new(),
        };
        state.player = state.spawn_player(origin, player_at)?;
        state.stock_quivers()?;
        fov::update(&mut state);
        Ok(state)
    }

    /// Creates what `spawn` describes and registers it and its equipment with
    /// the scheduler: a `SpawnSeq` each, for tie-breaking and save order,
    /// and, for anything with a `Speed`, `Energy(0)` so it can act on the
    /// tick it appears. The only way to create entities (ADR 0008); clippy
    /// rejects `World::spawn` elsewhere in `game`.
    pub(crate) fn spawn(&mut self, mut spawn: Spawn) -> Result<Entity, BlueprintError> {
        let entity = match spawn.source {
            Source::Blueprint(name) => {
                let id = self
                    .blueprints
                    .id(&name)
                    .ok_or(BlueprintError::UnknownBlueprint(name))?;
                self.blueprints.spawn_id(&mut self.world, id)?
            }
            Source::Id(id) => self.blueprints.spawn_id(&mut self.world, id)?,
            #[cfg(test)]
            Source::Blank => self.spawn_blank(),
        };
        // Extras go in before registration, so a `Speed` among them counts.
        let _ = self.world.insert(entity, spawn.components.build());
        self.register(entity);
        #[cfg(test)]
        let _ = self.world.insert(entity, spawn.overrides.build());
        crate::health::start(self, entity);
        crate::rest::start(self, entity);

        let world = &mut self.world;
        match spawn.held {
            Some(Held::Attached(owner)) => holding::attach(world, owner, entity),
            #[cfg(test)]
            Some(Held::Equipped(owner, part)) => holding::put_on(world, owner, entity, part),
            Some(Held::Carried(owner)) => holding::stow(world, owner, entity),
            None => {}
        }
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
        for item in holding::attached(&self.world, entity) {
            self.register(item);
        }
    }

    #[cfg(test)]
    #[expect(
        clippy::disallowed_methods,
        reason = "the blank half of GameState::spawn, which registers it"
    )]
    fn spawn_blank(&mut self) -> Entity {
        self.world.spawn(())
    }
}

/// Every event handler in the game. Handlers are code, so a loaded save
/// rebuilds this rather than persisting it.
pub(crate) fn event_registry() -> EventRegistry {
    let mut events = EventRegistry::new();
    combat::register(&mut events);
    spell::register(&mut events);
    crate::style::register(&mut events);
    crate::technique::register(&mut events);
    #[cfg(test)]
    crate::testkit::register(&mut events);
    events
}

/// A `width` × `height` view through the camera following the player
/// (`camera::camera`). Visible tiles show terrain and whatever stands there;
/// remembered tiles show the terrain and the item last seen there, dimmed;
/// never-seen tiles and padding outside the zone stay blank. The player's
/// held light is baked into the colours (`light`).
pub fn render_view(state: &GameState, width: i32, height: i32) -> Grid<Cell> {
    let (mut cells, light) = crate::light::render_layers(state, width, height);
    for y in 0..cells.height {
        for x in 0..cells.width {
            if let (Some(cell), Some(Some(lit))) = (cells.get_mut(x, y), light.get(x, y)) {
                cell.fg = crate::light::tint(cell.fg, *lit);
                cell.bg = crate::light::tint(cell.bg, *lit);
            }
        }
    }
    cells
}

/// `render_view` before light.
pub(crate) fn render_unlit(state: &GameState, width: i32, height: i32) -> Grid<Cell> {
    let camera = camera(state, width, height);
    let mut grid = Grid::new(
        camera.width,
        camera.height,
        Cell {
            glyph: ' ',
            fg: Rgb(200, 200, 200),
            bg: Rgb(0, 0, 0),
            sprite: None,
        },
    );
    let tiles = state.blueprints.tiles();
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
                Tile::StairsDown => ('>', Rgb(240, 220, 80)),
                Tile::StairsUp => ('<', Rgb(240, 220, 80)),
                Tile::Door => ('+', Rgb(170, 120, 60)),
                Tile::DoorOpen => ('\'', Rgb(170, 120, 60)),
                Tile::Chasm => (':', Rgb(70, 70, 120)),
                Tile::Grass => ('"', Rgb(70, 150, 60)),
                Tile::Rubble => (',', Rgb(150, 130, 100)),
                Tile::DeepWater => ('~', Rgb(60, 110, 200)),
            };
            let remembered = state
                .remembered_items
                .get(x, y)
                .and_then(Option::as_ref)
                .filter(|_| !visible);
            let (glyph, fg) = remembered.map_or((glyph, fg), |look| (look.glyph, look.fg));
            if let Some(cell) = grid.get_mut(screen_x, screen_y) {
                cell.glyph = glyph;
                cell.fg = if visible { fg } else { dim(fg) };
                cell.sprite = tiles.terrain(tile).filter(|_| visible);
            }
        }
    }

    // Tiles a wind-up will strike: marked, so stepping out is a choice
    // (docs/plans/fights.md).
    for (x, y) in crate::technique::telegraphed(state) {
        let (screen_x, screen_y) = camera.world_to_screen((x, y));
        if let Some(cell) = grid.get_mut(screen_x, screen_y) {
            cell.bg = TELEGRAPH_BG;
        }
    }

    let mut shown: Vec<(Position, Cell, u8)> = state
        .world
        .query::<(
            &Position,
            &Appearance,
            Option<&Item>,
            Option<&crate::entity::Health>,
        )>()
        .iter()
        .filter(|(pos, ..)| {
            flag(&state.visible, pos.x, pos.y) && !crate::fov::hidden_in_grass(state, **pos)
        })
        .map(|(pos, look, item, creature)| {
            let shows = Cell {
                glyph: look.glyph,
                fg: look.fg,
                bg: look.bg,
                sprite: look.sprite.as_deref().and_then(|name| tiles.id(name)),
            };
            // Drawn in this order: items, then things like a ruin, then
            // creatures, so anyone standing on one is drawn over it.
            let layer = match (item, creature) {
                (Some(_), _) => 0,
                (None, None) => 1,
                (None, Some(_)) => 2,
            };
            (*pos, shows, layer)
        })
        .collect();
    shown.sort_by_key(|&(_, _, layer)| layer);
    for (pos, shows, _) in shown {
        let (screen_x, screen_y) = camera.world_to_screen((pos.x, pos.y));
        if let Some(cell) = grid.get_mut(screen_x, screen_y) {
            cell.glyph = shows.glyph;
            cell.fg = shows.fg;
            // No sprite of its own: its glyph, not the ground's sprite.
            cell.sprite = shows.sprite;
        }
    }

    grid
}

/// The ground a wind-up will strike.
pub const TELEGRAPH_BG: Rgb = Rgb(110, 20, 20);

/// Remembered terrain: half brightness.
fn dim(Rgb(r, g, b): Rgb) -> Rgb {
    Rgb(r / 2, g / 2, b / 2)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::blueprint::{Blueprint, ComponentDef, SummonDef};
    use crate::combat::roll_melee;
    use crate::entity::{
        Area, Armor, Attachments, Bleeding, BodyPart, DamageTypes, Duration, Equippable, Found,
        Health, Hits, Light, Melee, Name, PartKind, School, Shape, Spell, SpellDamage,
    };
    use crate::input::{Dir, PlayerAction};
    use crate::map::tests::room;
    use crate::testkit::{last_log, thing};
    use rand::Rng;

    fn blueprint(components: Vec<ComponentDef>, equipment: &[&str]) -> Blueprint {
        Blueprint {
            inherits: None,
            components,
            equipment: Some(equipment.iter().map(|s| s.to_string()).collect()),
            spells: None,
            techniques: None,
        }
    }

    fn look(glyph: char) -> ComponentDef {
        ComponentDef::Appearance(Appearance {
            glyph,
            fg: Rgb(255, 255, 255),
            bg: Rgb(0, 0, 0),
            sprite: None,
        })
    }

    /// Head, body, two hands: enough anatomy for the test items.
    pub(crate) fn test_body() -> ComponentDef {
        let part = |kind, name: &str| BodyPart {
            kind,
            name: name.to_string(),
        };
        ComponentDef::Body(crate::entity::Body(vec![
            part(PartKind::Head, "head"),
            part(PartKind::Body, "body"),
            part(PartKind::Hand, "right hand"),
            part(PartKind::Hand, "left hand"),
        ]))
    }

    /// Player: 20 HP, 2-4 melee. Goblin: 8 HP wearing 1-AV armor, hostile.
    /// Both normal speed. Bleeding: 1 damage every 100 ticks for 500 ticks.
    pub(crate) fn test_blueprints() -> Blueprints {
        let name = |n: &str| ComponentDef::Name(Name(n.to_string()));
        Blueprints::resolve_with(
            BTreeMap::from([
                (
                    "Player".to_string(),
                    blueprint(
                        vec![
                            name("player"),
                            ComponentDef::Player,
                            test_body(),
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
                            test_body(),
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
                    "Firebolt".to_string(),
                    blueprint(
                        vec![
                            name("firebolt"),
                            ComponentDef::Spell(Spell {
                                school: School::Pyromancy,
                                range: 7,
                                power: 2,
                            }),
                            ComponentDef::SpellDamage(SpellDamage {
                                min: 3,
                                max: 6,
                                types: DamageTypes::default(),
                            }),
                        ],
                        &[],
                    ),
                ),
                (
                    "MagicMissile".to_string(),
                    blueprint(
                        vec![
                            name("magic missile"),
                            ComponentDef::Spell(Spell {
                                school: School::Arcane,
                                range: 8,
                                power: 1,
                            }),
                            ComponentDef::SpellDamage(SpellDamage {
                                min: 2,
                                max: 4,
                                types: DamageTypes::default(),
                            }),
                        ],
                        &[],
                    ),
                ),
                (
                    "Fireball".to_string(),
                    blueprint(
                        vec![
                            name("fireball"),
                            ComponentDef::Spell(Spell {
                                school: School::Pyromancy,
                                range: 6,
                                power: 4,
                            }),
                            ComponentDef::SpellDamage(SpellDamage {
                                min: 2,
                                max: 5,
                                types: DamageTypes::default(),
                            }),
                            ComponentDef::Area(Area {
                                shape: Shape::Burst { radius: 1 },
                                hits: Hits::All,
                                falloff: 0,
                            }),
                        ],
                        &[],
                    ),
                ),
                (
                    "ArcaneLance".to_string(),
                    blueprint(
                        vec![
                            name("arcane lance"),
                            ComponentDef::Spell(Spell {
                                school: School::Arcane,
                                range: 8,
                                power: 2,
                            }),
                            ComponentDef::SpellDamage(SpellDamage {
                                min: 2,
                                max: 4,
                                types: DamageTypes::default(),
                            }),
                            ComponentDef::Area(Area {
                                shape: Shape::Beam,
                                hits: Hits::Enemies,
                                falloff: 0,
                            }),
                        ],
                        &[],
                    ),
                ),
                (
                    "SummonWolf".to_string(),
                    blueprint(
                        vec![
                            name("summon wolf"),
                            ComponentDef::Spell(Spell {
                                school: School::Arcane,
                                range: 5,
                                power: 3,
                            }),
                            ComponentDef::Summon(SummonDef {
                                blueprint: "SpiritWolf".to_string(),
                                count: 1,
                                lifetime: 3000,
                            }),
                        ],
                        &[],
                    ),
                ),
                (
                    "SpiritWolf".to_string(),
                    blueprint(
                        vec![
                            name("spirit wolf"),
                            ComponentDef::BlocksMovement,
                            ComponentDef::Health(Health { current: 8, max: 8 }),
                            ComponentDef::Melee(Melee { min: 2, max: 4 }),
                            ComponentDef::Speed(Speed(120)),
                        ],
                        &[],
                    ),
                ),
                (
                    "Binding".to_string(),
                    blueprint(vec![ComponentDef::Binding], &[]),
                ),
                (
                    "Rune".to_string(),
                    blueprint(
                        vec![ComponentDef::Appearance(Appearance {
                            glyph: 'o',
                            fg: Rgb(255, 120, 40),
                            bg: Rgb(0, 0, 0),
                            sprite: None,
                        })],
                        &[],
                    ),
                ),
                (
                    "GoblinShaman".to_string(),
                    blueprint(
                        vec![
                            name("goblin shaman"),
                            ComponentDef::BlocksMovement,
                            ComponentDef::Health(Health {
                                current: 10,
                                max: 10,
                            }),
                            ComponentDef::Melee(Melee { min: 1, max: 3 }),
                            ComponentDef::Speed(Speed(100)),
                            ComponentDef::Hostile,
                        ],
                        &[],
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
                        vec![
                            name("leather armor"),
                            look('['),
                            ComponentDef::Item,
                            ComponentDef::Equippable(Equippable {
                                part: PartKind::Body,
                            }),
                            ComponentDef::Armor(Armor::uniform(1)),
                        ],
                        &[],
                    ),
                ),
            ]),
            crate::skill::tests::book(),
        )
        .and_then(|blueprints| {
            blueprints
                .with_phases(crate::phase::tests::test_phases())
                .with_dungeons(BTreeMap::from([(
                    "Test".to_string(),
                    crate::dungeon::tests::test_dungeon(),
                )]))
        })
        .expect("test blueprints resolve")
    }

    /// The hand-built `room` with the player at (1, 1) and nothing else, so
    /// tests control every other entity and generator tuning can't move
    /// walls under them.
    pub(crate) fn bare_state() -> GameState {
        bare_state_seeded(1)
    }

    /// `bare_state` with other content, such as the real `data/`.
    pub(crate) fn bare_state_with(blueprints: Blueprints) -> GameState {
        GameState::with_map(
            1,
            Rngs::new(1, 1),
            room(),
            Position { x: 1, y: 1 },
            blueprints,
            None,
        )
        .unwrap()
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
            None,
        )
        .unwrap()
    }

    pub(crate) fn player_position(state: &GameState) -> Position {
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
            let state = GameState::new(seed, test_blueprints()).unwrap();
            let player = player_position(&state);
            let goblins = goblins(&state);
            let expected = crate::dungeon::tests::test_dungeon().creatures[0].count;
            assert_eq!(goblins.len(), expected, "seed {seed}");
            for (at, gear) in &goblins {
                assert_eq!(state.map.get(at.x, at.y), Some(&Tile::Floor));
                let distance = (at.x - player.x).abs().max((at.y - player.y).abs());
                assert!(distance >= CREATURE_MIN_DISTANCE, "seed {seed}: {at:?}");
                assert!(state.world.satisfies::<&Armor>(gear[0]));
            }
        }
    }

    #[test]
    fn new_scatters_unfound_items_and_places_a_reachable_exit() {
        for seed in 0..5 {
            let state = GameState::new(seed, test_blueprints()).unwrap();
            let player = player_position(&state);
            let items: Vec<Position> = state
                .world
                .query::<(&Position, &Item, Option<&Found>)>()
                .iter()
                .map(|(pos, _, found)| {
                    assert!(found.is_none(), "seed {seed}");
                    *pos
                })
                .collect();
            let expected = crate::dungeon::tests::test_dungeon().items[0].count;
            assert_eq!(items.len(), expected, "seed {seed}");
            for at in &items {
                assert_eq!(state.map.get(at.x, at.y), Some(&Tile::Floor));
                let distance = (at.x - player.x).abs().max((at.y - player.y).abs());
                assert!(distance >= ITEM_MIN_DISTANCE, "seed {seed}: {at:?}");
            }
            let exits = state
                .map
                .cells()
                .iter()
                .filter(|&&t| t == Tile::StairsDown)
                .count();
            assert_eq!(exits, 1, "seed {seed}");
        }
        let a = GameState::new(9, test_blueprints()).unwrap();
        let b = GameState::new(9, test_blueprints()).unwrap();
        assert_eq!(a.map, b.map);
    }

    #[test]
    fn the_player_starts_on_the_stairs_up_nearest_the_centre() {
        let state = GameState::bare(3, test_blueprints()).unwrap();
        let start = player_position(&state);
        assert_eq!(state.map.get(start.x, start.y), Some(&Tile::StairsUp));
        let (cx, cy) = (state.map.width / 2, state.map.height / 2);
        let dist = |x: i32, y: i32| (x - cx).pow(2) + (y - cy).pow(2);
        let nearest_floor = crate::map::floor_candidates(&state.map, &[])
            .into_iter()
            .map(|(x, y)| dist(x, y))
            .min()
            .unwrap();
        assert!(dist(start.x, start.y) <= nearest_floor);
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
            .map(|_| {
                roll_melee(
                    &state.world,
                    state.blueprints.skills(),
                    &mut state.rngs.combat,
                    state.player,
                )
                .unwrap()
                .amount
            })
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

        let mut perturbed = GameState::bare(9, test_blueprints()).unwrap();
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
            let state = GameState::new(seed, test_blueprints()).unwrap();
            let tiles: Vec<Position> = positions(&state).into_iter().map(|(p, _)| p).collect();
            for (i, pos) in tiles.iter().enumerate() {
                assert!(Tile::is_open(state.map.get(pos.x, pos.y)), "seed {seed}");
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
        state.run(Command::TryMove(state.player, Dir::E));
        assert_eq!(player_position(&state), Position { x: 2, y: 1 });
    }

    #[test]
    fn move_into_wall_is_blocked_and_logs_a_message() {
        let mut state = bare_state();
        // (1,1) -> north is (1,0), a wall in `room`.
        state.run(Command::TryMove(state.player, Dir::N));
        assert_eq!(player_position(&state), Position { x: 1, y: 1 });
        assert_eq!(last_log(&state), "You bump into a wall.");
    }

    #[test]
    fn move_into_blocking_entity_is_blocked_and_logs_a_message() {
        let mut state = bare_state();
        state.spawn(Spawn::blueprint("Rubble").at(2, 1)).unwrap();
        state.run(Command::TryMove(state.player, Dir::E));
        assert_eq!(player_position(&state), Position { x: 1, y: 1 });
        assert_eq!(last_log(&state), "Something blocks your way.");
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
        state.spawn(Spawn::blueprint("Goblin").at(2, 1)).unwrap();

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
    fn a_held_torch_sees_further_and_warms_what_it_lights() {
        let mut state = bare_state();
        let player = state.player;
        let torch = thing("torch")
            .item()
            .equipped_on(player, 4)
            .make(&mut state);
        let light = Light {
            radius: 11,
            tint: Rgb(255, 150, 50),
            tint_bp: 2_000,
        };
        state.world.insert_one(torch, light).unwrap();
        crate::time::take_turn(&mut state, PlayerAction::Wait);

        // Plain sight stops at 8 tiles; the torch reaches 11.
        assert_eq!(player_position(&state), Position { x: 1, y: 1 });
        assert_eq!(state.visible.get(12, 1), Some(&true));
        assert_eq!(state.visible.get(13, 1), Some(&false));

        let view = render_view(&state, 40, 20);
        let lit = view.get(10, 1).unwrap();
        // Nine tiles out of eleven, the glow is down to 875 of its 2 000.
        assert_eq!((lit.fg, lit.bg), (Rgb(104, 95, 87), Rgb(22, 13, 4)));
        let player_cell = view.get(1, 1).unwrap();
        assert_ne!(player_cell.fg, Rgb(255, 255, 255));

        // Out of hand, it lights nothing.
        crate::equipment::unequip(&mut state, player, torch);
        crate::time::take_turn(&mut state, PlayerAction::Wait);
        assert_eq!(state.visible.get(12, 1), Some(&false));
        let unlit = *render_view(&state, 40, 20).get(5, 1).unwrap();
        assert_eq!((unlit.fg, unlit.bg), (Rgb(90, 90, 90), Rgb(0, 0, 0)));
    }

    fn knife_at(state: &mut GameState, x: i32, y: i32) -> Entity {
        thing("knife")
            .look('/', Rgb(200, 100, 50))
            .item()
            .at(x, y)
            .make(state)
    }

    #[test]
    fn items_seen_are_remembered_dimmed_out_of_view() {
        let mut state = bare_state();
        knife_at(&mut state, 2, 1);
        // Spawned after the starting view; it is seen on the first turn.
        for _ in 0..10 {
            crate::time::take_turn(&mut state, PlayerAction::Move(Dir::E));
        }
        assert_eq!(state.visible.get(2, 1), Some(&false));

        let remembered = *render_view(&state, 40, 20).get(2, 1).unwrap();
        assert_eq!((remembered.glyph, remembered.fg), ('/', Rgb(100, 50, 25)));
    }

    #[test]
    fn item_memory_is_only_updated_by_seeing_the_tile() {
        let mut state = bare_state();
        let knife = knife_at(&mut state, 2, 1);
        for _ in 0..10 {
            crate::time::take_turn(&mut state, PlayerAction::Move(Dir::E));
        }
        // Moved out of view: the old spot is still remembered, the new one
        // unknown.
        state
            .world
            .insert_one(knife, Position { x: 2, y: 2 })
            .unwrap();
        crate::time::take_turn(&mut state, PlayerAction::Wait);
        let view = render_view(&state, 40, 20);
        assert_eq!(view.get(2, 1).map(|cell| cell.glyph), Some('/'));
        assert_eq!(view.get(2, 2).map(|cell| cell.glyph), Some('.'));

        // Seeing the tiles again corrects the memory.
        for _ in 0..10 {
            crate::time::take_turn(&mut state, PlayerAction::Move(Dir::W));
        }
        assert_eq!(state.remembered_items.get(2, 1), Some(&None));
        assert!(
            state
                .remembered_items
                .get(2, 2)
                .is_some_and(Option::is_some)
        );
    }

    #[test]
    fn the_view_is_the_viewport_size_and_centred_on_the_player() {
        let state = GameState::bare(3, test_blueprints()).unwrap();
        let view = render_view(&state, 41, 21);
        assert_eq!((view.width, view.height), (41, 21));
        assert_eq!(view.get(20, 10).map(|cell| cell.glyph), Some('@'));
    }

    #[test]
    fn diagonal_move_succeeds() {
        let mut state = bare_state();
        state.run(Command::TryMove(state.player, Dir::SE));
        assert_eq!(player_position(&state), Position { x: 2, y: 2 });
    }
}
