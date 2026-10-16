//! Save games (ADR 0008): `GameState` to and from a RON string. File IO is
//! the frontend's job; this module only converts.
//!
//! Entities are written as lists of component values in a canonical order,
//! and every `Entity` reference as an index into that list, so a save never
//! depends on hecs ids. Blueprint references are written as names.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use hecs::{Component, Entity, EntityBuilder, EntityRef, World};
use ron::extensions::Extensions;
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

use crate::blueprint::{BlueprintId, Blueprints, OnHitDef};
use crate::entity::{
    Appearance, Armor, Attachments, Bleeding, BlocksMovement, Body, Dead, Duration, EffectTarget,
    Energy, Equippable, EquippedOn, Found, Health, Hostile, Inventory, Item, Melee, Name, OnHit,
    OnHitEntry, Player, Position, SpawnSeq, Speed, Stats, Throwable, Timed, Weapon,
};
use crate::explore::ExploreItems;
use crate::fov;
use crate::map::Grid;
use crate::rng::Rngs;
use crate::state::{GameState, MessageLog, event_registry};
use crate::time::{Tick, Timer, Timers};

/// Bumped on any change to the format. Older saves are refused, not migrated.
pub const SAVE_VERSION: u32 = 9;

#[derive(Debug)]
pub enum SaveError {
    /// Saving mid-turn would lose queued commands; save between player turns.
    CommandsPending(usize),
    /// The entity at this save index has a component the format doesn't
    /// know. Add it to `ComponentKind` and `SavedComponent`.
    UnknownComponent(u32),
    /// This many entities have no `SpawnSeq`, so they have no place in the
    /// save order: they were created outside `GameState::spawn`.
    MissingSpawnSeq(usize),
    /// A reference to an entity that no longer exists, where one must.
    DanglingReference,
    UnknownBlueprintId(BlueprintId),
    Ron(ron::Error),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandsPending(n) => {
                write!(f, "cannot save mid-turn: {n} command(s) still queued")
            }
            Self::UnknownComponent(index) => {
                write!(
                    f,
                    "entity {index} has a component the save format doesn't know"
                )
            }
            Self::MissingSpawnSeq(n) => write!(
                f,
                "{n} entity(ies) have no SpawnSeq; entities must be created with GameState::spawn"
            ),
            Self::DanglingReference => write!(f, "an entity refers to one that no longer exists"),
            Self::UnknownBlueprintId(id) => write!(f, "no blueprint with id {id:?}"),
            Self::Ron(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SaveError {}

#[derive(Debug)]
pub enum LoadError {
    Parse(ron::error::SpannedError),
    Version {
        found: u32,
        expected: u32,
    },
    UnknownBlueprint(String),
    BadEntityIndex(u32),
    BadGrid(&'static str),
    /// The entity at this save index has no `SpawnSeq` (ADR 0008).
    MissingSpawnSeq(u32),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "malformed save: {err}"),
            Self::Version { found, expected } => write!(
                f,
                "save format version {found} is not supported (this build reads version {expected})"
            ),
            Self::UnknownBlueprint(name) => {
                write!(f, "save refers to unknown blueprint {name:?}")
            }
            Self::BadEntityIndex(index) => write!(f, "save refers to missing entity {index}"),
            Self::BadGrid(which) => write!(f, "save has a malformed {which} grid"),
            Self::MissingSpawnSeq(index) => write!(f, "saved entity {index} has no SpawnSeq"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Read first and alone, so a save from another version fails with a
/// version error instead of whatever parse error its layout would cause.
#[derive(Deserialize)]
struct Header {
    version: u32,
}

#[derive(Serialize, Deserialize)]
struct Save {
    version: u32,
    seed: u64,
    now: Tick,
    game_over: bool,
    player: u32,
    next_spawn_seq: u64,
    explore_items: ExploreItems,
    rngs: Rngs,
    map: SavedGrid<crate::map::Tile>,
    seen: SavedGrid<bool>,
    remembered_items: SavedGrid<Option<Appearance>>,
    log: MessageLog,
    timers: SavedTimers,
    entities: Vec<Vec<SavedComponent>>,
}

#[derive(Serialize, Deserialize)]
struct SavedGrid<T> {
    width: i32,
    height: i32,
    cells: Vec<T>,
}

#[derive(Serialize, Deserialize)]
struct SavedTimers {
    next_seq: u64,
    /// In firing order; `seq` keeps same-tick order.
    queue: Vec<SavedTimer>,
}

#[derive(Serialize, Deserialize)]
struct SavedTimer {
    at: Tick,
    seq: u64,
    timer: SavedTimerKind,
}

/// `None`: the timer's entity is already gone. Such a timer is a queued
/// no-op, kept so the queue (and the clock jumps it causes) is unchanged.
#[derive(Serialize, Deserialize)]
enum SavedTimerKind {
    ExpireEffect(Option<u32>),
    BleedTick(Option<u32>),
}

/// Every component type. Saving walks `ALL`; an entity with a component not
/// covered here fails to save rather than losing it.
#[derive(Debug, Clone, Copy)]
enum ComponentKind {
    Position,
    Appearance,
    BlocksMovement,
    Player,
    Dead,
    Name,
    Health,
    Armor,
    Melee,
    Attachments,
    Inventory,
    Speed,
    Energy,
    SpawnSeq,
    Hostile,
    Item,
    Found,
    Body,
    Equippable,
    EquippedOn,
    Stats,
    Weapon,
    Throwable,
    Duration,
    Timed,
    EffectTarget,
    Bleeding,
    OnHit,
}

impl ComponentKind {
    const ALL: [ComponentKind; 28] = [
        Self::Position,
        Self::Appearance,
        Self::BlocksMovement,
        Self::Player,
        Self::Dead,
        Self::Name,
        Self::Health,
        Self::Armor,
        Self::Melee,
        Self::Attachments,
        Self::Inventory,
        Self::Speed,
        Self::Energy,
        Self::SpawnSeq,
        Self::Hostile,
        Self::Item,
        Self::Found,
        Self::Body,
        Self::Equippable,
        Self::EquippedOn,
        Self::Stats,
        Self::Weapon,
        Self::Throwable,
        Self::Duration,
        Self::Timed,
        Self::EffectTarget,
        Self::Bleeding,
        Self::OnHit,
    ];
}

/// A component as saved: plain values as-is, entity references as save
/// indices, blueprint references as names.
#[derive(Debug, Serialize, Deserialize)]
enum SavedComponent {
    Position(Position),
    Appearance(Appearance),
    BlocksMovement,
    Player,
    Dead,
    Name(Name),
    Health(Health),
    Armor(Armor),
    Melee(Melee),
    Attachments(Vec<u32>),
    Inventory(Vec<u32>),
    Speed(Speed),
    Energy(Energy),
    SpawnSeq(SpawnSeq),
    Hostile,
    Item,
    Found,
    Body(Body),
    Equippable(Equippable),
    EquippedOn(EquippedOn),
    Stats(Stats),
    Weapon(Weapon),
    Throwable(Throwable),
    Duration(Duration),
    Timed(Timed),
    EffectTarget(u32),
    Bleeding(Bleeding),
    OnHit(Vec<OnHitDef>),
}

/// Serializes `state`. Only valid between player turns, with the command
/// queue empty.
pub fn to_ron(state: &GameState) -> Result<String, SaveError> {
    if !state.commands.is_empty() {
        return Err(SaveError::CommandsPending(state.commands.len()));
    }

    let order = save_order(&state.world)?;
    let index: BTreeMap<Entity, u32> = order.iter().copied().zip(0..).collect();
    let refer = |entity: Entity| index.get(&entity).copied();
    let must_refer = |entity: Entity| refer(entity).ok_or(SaveError::DanglingReference);

    let mut entities = Vec::with_capacity(order.len());
    for (i, &entity) in (0..).zip(&order) {
        let entity = state
            .world
            .entity(entity)
            .map_err(|_| SaveError::DanglingReference)?;
        let mut components = Vec::new();
        for kind in ComponentKind::ALL {
            if let Some(component) = save_component(kind, entity, &must_refer, &state.blueprints)? {
                components.push(component);
            }
        }
        if components.len() != entity.len() {
            return Err(SaveError::UnknownComponent(i));
        }
        entities.push(components);
    }

    let queue = state
        .timers
        .queue
        .iter()
        .map(|(&(at, seq), &timer)| SavedTimer {
            at,
            seq,
            timer: match timer {
                Timer::ExpireEffect(e) => SavedTimerKind::ExpireEffect(refer(e)),
                Timer::BleedTick(e) => SavedTimerKind::BleedTick(refer(e)),
            },
        })
        .collect();

    let save = Save {
        version: SAVE_VERSION,
        seed: state.seed,
        now: state.now,
        game_over: state.game_over,
        player: must_refer(state.player)?,
        next_spawn_seq: state.next_spawn_seq,
        explore_items: state.explore_items,
        rngs: state.rngs.clone(),
        map: saved_grid(&state.map),
        seen: saved_grid(&state.seen),
        remembered_items: saved_grid(&state.remembered_items),
        log: state.log.clone(),
        timers: SavedTimers {
            next_seq: state.timers.next_seq,
            queue,
        },
        entities,
    };
    let config = PrettyConfig::new().extensions(Extensions::UNWRAP_VARIANT_NEWTYPES);
    ron::ser::to_string_pretty(&save, config).map_err(SaveError::Ron)
}

/// Rebuilds a `GameState` from `to_ron` output, resolving blueprint names
/// against `blueprints`. The visible set is recomputed, not loaded.
pub fn from_ron(text: &str, blueprints: Blueprints) -> Result<GameState, LoadError> {
    let header: Header = ron::from_str(text).map_err(LoadError::Parse)?;
    if header.version != SAVE_VERSION {
        return Err(LoadError::Version {
            found: header.version,
            expected: SAVE_VERSION,
        });
    }
    let save: Save = ron::from_str(text).map_err(LoadError::Parse)?;

    let mut world = World::new();
    #[expect(
        clippy::disallowed_methods,
        reason = "restores saved entities, SpawnSeq included; nothing new is created"
    )]
    let handles: Vec<Entity> = save.entities.iter().map(|_| world.spawn(())).collect();
    let entity = |index: u32| {
        handles
            .get(index as usize)
            .copied()
            .ok_or(LoadError::BadEntityIndex(index))
    };

    for ((&handle, components), index) in handles.iter().zip(save.entities).zip(0..) {
        if !components
            .iter()
            .any(|component| matches!(component, SavedComponent::SpawnSeq(_)))
        {
            return Err(LoadError::MissingSpawnSeq(index));
        }
        let mut builder = EntityBuilder::new();
        for component in components {
            load_component(component, &mut builder, &entity, &blueprints)?;
        }
        // `handle` was spawned just above.
        let _ = world.insert(handle, builder.build());
    }

    let timer_entity = |index: Option<u32>| match index {
        Some(index) => entity(index),
        None => Ok(Entity::DANGLING),
    };
    let mut queue = BTreeMap::new();
    for SavedTimer { at, seq, timer } in save.timers.queue {
        let timer = match timer {
            SavedTimerKind::ExpireEffect(e) => Timer::ExpireEffect(timer_entity(e)?),
            SavedTimerKind::BleedTick(e) => Timer::BleedTick(timer_entity(e)?),
        };
        queue.insert((at, seq), timer);
    }

    let map = load_grid(save.map).ok_or(LoadError::BadGrid("map"))?;
    let seen = load_grid(save.seen).ok_or(LoadError::BadGrid("seen"))?;
    if (seen.width, seen.height) != (map.width, map.height) {
        return Err(LoadError::BadGrid("seen"));
    }
    let remembered_items = load_grid(save.remembered_items)
        .filter(|grid| (grid.width, grid.height) == (map.width, map.height))
        .ok_or(LoadError::BadGrid("remembered_items"))?;
    let visible = Grid::new(map.width, map.height, false);

    let mut state = GameState {
        world,
        map,
        log: save.log,
        rngs: save.rngs,
        commands: VecDeque::new(),
        events: event_registry(),
        blueprints,
        player: entity(save.player)?,
        seed: save.seed,
        now: save.now,
        timers: Timers {
            queue,
            next_seq: save.timers.next_seq,
        },
        game_over: save.game_over,
        visible,
        seen,
        remembered_items,
        next_spawn_seq: save.next_spawn_seq,
        explore_items: save.explore_items,
    };
    fov::update(&mut state);
    Ok(state)
}

/// Entities in `SpawnSeq` order, each followed depth-first by what it
/// carries, so the order depends only on game state, never on hecs ids or
/// archetype layout. Every entity must have a `SpawnSeq` (ADR 0008).
fn save_order(world: &World) -> Result<Vec<Entity>, SaveError> {
    let unsequenced = world
        .iter()
        .filter(|entity| !entity.has::<SpawnSeq>())
        .count();
    if unsequenced > 0 {
        return Err(SaveError::MissingSpawnSeq(unsequenced));
    }
    let mut sequenced: Vec<(SpawnSeq, Entity)> = world
        .query::<(Entity, &SpawnSeq)>()
        .iter()
        .map(|(entity, &seq)| (seq, entity))
        .collect();
    sequenced.sort();

    let mut order = Vec::new();
    let mut visited = BTreeSet::new();
    for (_, root) in sequenced {
        visit(world, root, &mut order, &mut visited);
    }
    Ok(order)
}

fn visit(world: &World, entity: Entity, order: &mut Vec<Entity>, visited: &mut BTreeSet<Entity>) {
    if !world.contains(entity) || !visited.insert(entity) {
        return;
    }
    order.push(entity);
    let mut carried = world
        .get::<&Attachments>(entity)
        .map(|attachments| attachments.0.clone())
        .unwrap_or_default();
    if let Ok(inventory) = world.get::<&Inventory>(entity) {
        carried.extend(inventory.0.iter().copied());
    }
    for item in carried {
        visit(world, item, order, visited);
    }
}

fn save_component(
    kind: ComponentKind,
    entity: EntityRef<'_>,
    refer: &impl Fn(Entity) -> Result<u32, SaveError>,
    blueprints: &Blueprints,
) -> Result<Option<SavedComponent>, SaveError> {
    fn copy<T: Component + Clone>(entity: EntityRef<'_>) -> Option<T> {
        entity.get::<&T>().map(|component| (*component).clone())
    }
    fn marker<T: Component>(
        entity: EntityRef<'_>,
        saved: SavedComponent,
    ) -> Option<SavedComponent> {
        entity.has::<T>().then_some(saved)
    }
    use SavedComponent as S;

    Ok(match kind {
        ComponentKind::Position => copy(entity).map(S::Position),
        ComponentKind::Appearance => copy(entity).map(S::Appearance),
        ComponentKind::BlocksMovement => marker::<BlocksMovement>(entity, S::BlocksMovement),
        ComponentKind::Player => marker::<Player>(entity, S::Player),
        ComponentKind::Dead => marker::<Dead>(entity, S::Dead),
        ComponentKind::Name => copy(entity).map(S::Name),
        ComponentKind::Health => copy(entity).map(S::Health),
        ComponentKind::Armor => copy(entity).map(S::Armor),
        ComponentKind::Melee => copy(entity).map(S::Melee),
        ComponentKind::Attachments => match copy::<Attachments>(entity) {
            Some(Attachments(items)) => Some(S::Attachments(
                items.into_iter().map(refer).collect::<Result<_, _>>()?,
            )),
            None => None,
        },
        ComponentKind::Inventory => match copy::<Inventory>(entity) {
            Some(Inventory(items)) => Some(S::Inventory(
                items.into_iter().map(refer).collect::<Result<_, _>>()?,
            )),
            None => None,
        },
        ComponentKind::Speed => copy(entity).map(S::Speed),
        ComponentKind::Energy => copy(entity).map(S::Energy),
        ComponentKind::SpawnSeq => copy(entity).map(S::SpawnSeq),
        ComponentKind::Hostile => marker::<Hostile>(entity, S::Hostile),
        ComponentKind::Item => marker::<Item>(entity, S::Item),
        ComponentKind::Found => marker::<Found>(entity, S::Found),
        ComponentKind::Body => copy(entity).map(S::Body),
        ComponentKind::Equippable => copy(entity).map(S::Equippable),
        ComponentKind::EquippedOn => copy(entity).map(S::EquippedOn),
        ComponentKind::Stats => copy(entity).map(S::Stats),
        ComponentKind::Weapon => copy(entity).map(S::Weapon),
        ComponentKind::Throwable => copy(entity).map(S::Throwable),
        ComponentKind::Duration => copy(entity).map(S::Duration),
        ComponentKind::Timed => copy(entity).map(S::Timed),
        ComponentKind::EffectTarget => match copy::<EffectTarget>(entity) {
            Some(EffectTarget(target)) => Some(S::EffectTarget(refer(target)?)),
            None => None,
        },
        ComponentKind::Bleeding => copy(entity).map(S::Bleeding),
        ComponentKind::OnHit => match copy::<OnHit>(entity) {
            Some(OnHit(entries)) => Some(S::OnHit(
                entries
                    .into_iter()
                    .map(|entry| {
                        let blueprint = blueprints
                            .by_id(entry.effect)
                            .ok_or(SaveError::UnknownBlueprintId(entry.effect))?;
                        Ok(OnHitDef {
                            effect: blueprint.name.clone(),
                            chance_bp: entry.chance_bp,
                        })
                    })
                    .collect::<Result<_, SaveError>>()?,
            )),
            None => None,
        },
    })
}

fn load_component(
    component: SavedComponent,
    builder: &mut EntityBuilder,
    entity: &impl Fn(u32) -> Result<Entity, LoadError>,
    blueprints: &Blueprints,
) -> Result<(), LoadError> {
    use SavedComponent as S;

    match component {
        S::Position(c) => builder.add(c),
        S::Appearance(c) => builder.add(c),
        S::BlocksMovement => builder.add(BlocksMovement),
        S::Player => builder.add(Player),
        S::Dead => builder.add(Dead),
        S::Name(c) => builder.add(c),
        S::Health(c) => builder.add(c),
        S::Armor(c) => builder.add(c),
        S::Melee(c) => builder.add(c),
        S::Attachments(items) => builder.add(Attachments(
            items.into_iter().map(entity).collect::<Result<_, _>>()?,
        )),
        S::Inventory(items) => builder.add(Inventory(
            items.into_iter().map(entity).collect::<Result<_, _>>()?,
        )),
        S::Speed(c) => builder.add(c),
        S::Energy(c) => builder.add(c),
        S::SpawnSeq(c) => builder.add(c),
        S::Hostile => builder.add(Hostile),
        S::Item => builder.add(Item),
        S::Found => builder.add(Found),
        S::Body(c) => builder.add(c),
        S::Equippable(c) => builder.add(c),
        S::EquippedOn(c) => builder.add(c),
        S::Stats(c) => builder.add(c),
        S::Weapon(c) => builder.add(c),
        S::Throwable(c) => builder.add(c),
        S::Duration(c) => builder.add(c),
        S::Timed(c) => builder.add(c),
        S::EffectTarget(target) => builder.add(EffectTarget(entity(target)?)),
        S::Bleeding(c) => builder.add(c),
        S::OnHit(entries) => builder.add(OnHit(
            entries
                .into_iter()
                .map(|entry| {
                    let effect = blueprints
                        .id(&entry.effect)
                        .ok_or(LoadError::UnknownBlueprint(entry.effect))?;
                    Ok(OnHitEntry {
                        effect,
                        chance_bp: entry.chance_bp,
                    })
                })
                .collect::<Result<_, LoadError>>()?,
        )),
    };
    Ok(())
}

fn saved_grid<T: Clone>(grid: &Grid<T>) -> SavedGrid<T> {
    SavedGrid {
        width: grid.width,
        height: grid.height,
        cells: grid.cells().to_vec(),
    }
}

fn load_grid<T>(saved: SavedGrid<T>) -> Option<Grid<T>> {
    Grid::from_cells(saved.width, saved.height, saved.cells)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::command::Command;
    use crate::entity::{DamageType, Stat};
    use crate::input::{Dir, PlayerAction};
    use crate::map::Tile;
    use crate::spawn::Spawn;
    use crate::state::GOBLINS;
    use crate::state::tests::{bare_state, test_blueprints};
    use crate::testkit::thing;
    use crate::time::{Step, step, take_turn};
    use crate::zone::CaveParams;

    fn data_blueprints() -> Blueprints {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/entities");
        Blueprints::load_dir(&dir).unwrap()
    }

    fn data_cave() -> CaveParams {
        CaveParams::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/cave.ron")).unwrap()
    }

    fn new_game(seed: u64) -> GameState {
        GameState::new(seed, data_blueprints(), &data_cave()).unwrap()
    }

    fn reload(state: &GameState, blueprints: Blueprints) -> GameState {
        from_ron(&to_ron(state).unwrap(), blueprints).unwrap()
    }

    fn goblin(state: &GameState) -> Option<Entity> {
        state
            .world
            .query::<(Entity, &Name)>()
            .iter()
            .find(|(_, name)| name.0 == "goblin")
            .map(|(entity, _)| entity)
    }

    fn goblins(state: &GameState) -> Vec<Position> {
        state
            .world
            .query::<(&Name, &Position)>()
            .without::<&Dead>()
            .iter()
            .filter(|(name, _)| name.0 == "goblin")
            .map(|(_, pos)| *pos)
            .collect()
    }

    /// Steps along a shortest path to the nearest goblin (attacking once
    /// adjacent), or waits. Decided from the state, so both runs issue the
    /// same inputs.
    fn scripted_turn(state: &mut GameState) {
        let toward = state
            .world
            .get::<&Position>(state.player)
            .ok()
            .and_then(|from| first_step((from.x, from.y), state, &goblins(state)));
        let action = match toward {
            Some(dir) => PlayerAction::Move(dir),
            None => PlayerAction::Wait,
        };
        take_turn(state, action);
    }

    /// Breadth-first over floor from `from` until a goblin tile; the first
    /// step of that path.
    fn first_step(from: (i32, i32), state: &GameState, goblins: &[Position]) -> Option<Dir> {
        let map = &state.map;
        let mut first: Grid<Option<Dir>> = Grid::new(map.width, map.height, None);
        let mut queue = std::collections::VecDeque::new();
        for dir in Dir::ALL {
            let (dx, dy) = dir.delta();
            let next = (from.0 + dx, from.1 + dy);
            if map.get(next.0, next.1) == Some(&Tile::Floor) {
                first.set(next.0, next.1, Some(dir));
                queue.push_back(next);
            }
        }
        while let Some((x, y)) = queue.pop_front() {
            let dir = first.get(x, y).copied().flatten()?;
            if goblins.contains(&Position { x, y }) {
                return Some(dir);
            }
            for next_dir in Dir::ALL {
                let (dx, dy) = next_dir.delta();
                let next = (x + dx, y + dy);
                if next != from
                    && map.get(next.0, next.1) == Some(&Tile::Floor)
                    && first.get(next.0, next.1) == Some(&None)
                {
                    first.set(next.0, next.1, Some(dir));
                    queue.push_back(next);
                }
            }
        }
        None
    }

    #[test]
    fn a_reloaded_game_continues_exactly_like_an_uninterrupted_one() {
        let (n, m) = (20, 130);
        let mut uninterrupted = new_game(42);
        for _ in 0..n + m {
            scripted_turn(&mut uninterrupted);
        }

        let mut first = new_game(42);
        for _ in 0..n {
            scripted_turn(&mut first);
        }
        let mut resumed = reload(&first, data_blueprints());
        for _ in 0..m {
            scripted_turn(&mut resumed);
        }

        assert!(
            goblins(&uninterrupted).len() < GOBLINS,
            "the script reaches combat"
        );
        assert_eq!(to_ron(&resumed).unwrap(), to_ron(&uninterrupted).unwrap());
        assert_eq!(resumed.log, uninterrupted.log);
        assert_eq!(resumed.now, uninterrupted.now);
    }

    #[test]
    fn saving_a_loaded_game_reproduces_the_save() {
        let mut state = new_game(7);
        for _ in 0..3 {
            scripted_turn(&mut state);
        }
        let saved = to_ron(&state).unwrap();
        let loaded = from_ron(&saved, data_blueprints()).unwrap();
        assert_eq!(to_ron(&loaded).unwrap(), saved);
        assert_eq!(loaded.visible, state.visible);
    }

    /// Steps the scheduler until no bleed is left; returns that tick.
    fn bleed_expiry(state: &mut GameState) -> Tick {
        while state.world.query::<&Bleeding>().iter().count() > 0 {
            if matches!(step(state), Step::PlayerReady) {
                take_turn(state, PlayerAction::Wait);
            }
        }
        state.now
    }

    #[test]
    fn a_carried_inventory_survives_a_round_trip() {
        let mut state = bare_state();
        for name in ["first", "second"] {
            thing(name).item().at(1, 1).make(&mut state);
        }
        take_turn(&mut state, PlayerAction::PickUp);
        let names = |state: &GameState| -> Vec<String> {
            crate::view::inventory_view(state)
                .into_iter()
                .map(|item| item.name)
                .collect()
        };
        assert_eq!(names(&state), ["first", "second"]);

        state.explore_items = crate::explore::ExploreItems::PickUp;
        let loaded = reload(&state, test_blueprints());
        assert_eq!(names(&loaded), ["first", "second"]);
        assert_eq!(loaded.explore_items, crate::explore::ExploreItems::PickUp);
        assert_eq!(to_ron(&loaded).unwrap(), to_ron(&state).unwrap());
    }

    #[test]
    fn a_throwable_item_and_its_damage_kinds_survive_a_round_trip() {
        let mut state = bare_state();
        let dagger = thing("dagger")
            .item()
            .throwable(
                6,
                (1, 2),
                Stat::Agility,
                &[DamageType::Slashing, DamageType::Piercing],
            )
            .carried_by(state.player)
            .make(&mut state);
        let before = *state.world.get::<&Throwable>(dagger).unwrap();

        let loaded = reload(&state, test_blueprints());
        let carried = crate::holding::carried(&loaded.world, loaded.player);
        assert_eq!(*loaded.world.get::<&Throwable>(carried[0]).unwrap(), before);
    }

    #[test]
    fn found_and_unfound_floor_items_survive_a_round_trip() {
        let mut state = bare_state();
        thing("found").item().found().at(4, 4).make(&mut state);
        thing("unfound").item().at(5, 4).make(&mut state);
        state.map.set(6, 4, Tile::Exit);

        let loaded = reload(&state, test_blueprints());
        let found: Vec<(String, bool)> = loaded
            .world
            .query::<(&Name, &Item, Option<&Found>)>()
            .iter()
            .map(|(name, _, found)| (name.0.clone(), found.is_some()))
            .collect();
        assert_eq!(
            found,
            [("found".to_string(), true), ("unfound".to_string(), false)]
        );
        assert_eq!(loaded.map.get(6, 4), Some(&Tile::Exit));
    }

    #[test]
    fn an_equipped_bleeding_goblin_survives_a_round_trip() {
        let mut state = bare_state();
        let goblin_entity = state.spawn(Spawn::blueprint("Goblin").at(12, 10)).unwrap();
        let blueprint = state.blueprints.id("Bleeding").unwrap();
        state.run(Command::ApplyEffect {
            target: goblin_entity,
            blueprint,
        });
        take_turn(&mut state, PlayerAction::Wait);

        let mut resumed = reload(&state, test_blueprints());

        let goblin_entity = goblin(&resumed).unwrap();
        let carried = crate::holding::attached(&resumed.world, goblin_entity);
        assert_eq!(carried.len(), 2);
        assert!(resumed.world.satisfies::<&Armor>(carried[0]));
        let bleed = carried[1];
        assert!(resumed.world.satisfies::<(&Bleeding, &Timed)>(bleed));
        assert_eq!(
            resumed.world.get::<&EffectTarget>(bleed).unwrap().0,
            goblin_entity
        );

        assert_eq!(bleed_expiry(&mut resumed), bleed_expiry(&mut state));
        assert_eq!(resumed.log, state.log);
    }

    #[test]
    fn a_save_from_another_version_is_refused() {
        let saved = to_ron(&bare_state()).unwrap();
        let other = saved.replacen(
            &format!("version: {SAVE_VERSION},"),
            &format!("version: {},", SAVE_VERSION + 1),
            1,
        );
        assert_ne!(other, saved);
        let err = from_ron(&other, test_blueprints()).err().unwrap();
        assert!(
            matches!(err, LoadError::Version { found, expected: SAVE_VERSION } if found == SAVE_VERSION + 1),
            "{err}"
        );

        let different_layout = "(version: 0, whatever: [1, 2])";
        let err = from_ron(different_layout, test_blueprints()).err().unwrap();
        assert!(matches!(err, LoadError::Version { found: 0, .. }), "{err}");
        assert_eq!(
            err.to_string(),
            format!(
                "save format version 0 is not supported (this build reads version {SAVE_VERSION})"
            )
        );
    }

    #[test]
    fn a_save_naming_an_unknown_blueprint_is_refused() {
        let saved = to_ron(&new_game(1)).unwrap();
        assert!(
            saved.contains("\"Bleeding\""),
            "the goblin's knife is saved"
        );
        let broken = saved.replace("\"Bleeding\"", "\"Nope\"");
        let err = from_ron(&broken, data_blueprints()).err().unwrap();
        assert!(matches!(&err, LoadError::UnknownBlueprint(name) if name == "Nope"));
        assert_eq!(err.to_string(), "save refers to unknown blueprint \"Nope\"");
    }

    #[test]
    fn saving_mid_turn_is_refused() {
        let mut state = bare_state();
        state.commands.push_back(Command::Wait(state.player));
        assert!(matches!(to_ron(&state), Err(SaveError::CommandsPending(1))));
    }

    #[test]
    fn an_entity_without_a_spawn_seq_fails_the_save() {
        let mut state = bare_state();
        let rock = thing("rock").make(&mut state);
        // Stands in for an entity created outside `GameState::spawn`.
        state.world.remove_one::<SpawnSeq>(rock).unwrap();
        let err = to_ron(&state).err().unwrap();
        assert!(matches!(err, SaveError::MissingSpawnSeq(1)));
        assert_eq!(
            err.to_string(),
            "1 entity(ies) have no SpawnSeq; entities must be created with GameState::spawn"
        );
    }

    #[test]
    fn spawned_equipment_gets_a_spawn_seq_after_its_owner() {
        let mut state = bare_state();
        let goblin = state.spawn(Spawn::blueprint("Goblin")).unwrap();
        let armor = state.world.get::<&Attachments>(goblin).unwrap().0[0];
        let seq = |entity| *state.world.get::<&SpawnSeq>(entity).unwrap();
        assert_eq!(seq(armor), SpawnSeq(seq(goblin).0 + 1));
    }

    #[test]
    fn a_save_with_an_entity_missing_its_spawn_seq_is_refused() {
        let saved = to_ron(&bare_state()).unwrap();
        let broken: String = saved
            .lines()
            .filter(|line| !line.trim_start().starts_with("SpawnSeq("))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_ne!(broken, saved);
        let err = from_ron(&broken, test_blueprints()).err().unwrap();
        assert!(matches!(err, LoadError::MissingSpawnSeq(0)));
        assert_eq!(err.to_string(), "saved entity 0 has no SpawnSeq");
    }

    #[test]
    fn a_component_the_format_does_not_know_fails_the_save() {
        let mut state = bare_state();
        state.world.insert_one(state.player, 7_u8).unwrap();
        assert!(matches!(
            to_ron(&state),
            Err(SaveError::UnknownComponent(0))
        ));
    }

    #[test]
    fn a_timer_for_a_despawned_entity_stays_queued() {
        let mut state = bare_state();
        let goblin_entity = state.spawn(Spawn::blueprint("Goblin").at(12, 10)).unwrap();
        let blueprint = state.blueprints.id("Bleeding").unwrap();
        state.commands.push_back(Command::ApplyEffect {
            target: goblin_entity,
            blueprint,
        });
        state.run(Command::Die {
            entity: goblin_entity,
        });
        let queued = state.timers.len();
        assert!(queued > 0);

        let loaded = reload(&state, test_blueprints());
        assert_eq!(loaded.timers.len(), queued);
        assert_eq!(loaded.timers.next_due(), state.timers.next_due());
    }
}
