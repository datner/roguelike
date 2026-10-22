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

use crate::blueprint::{BlueprintId, Blueprints, OnHitDef, SummonDef};
use crate::entity::{
    Ammo, Appearance, Area, Armor, Attachments, Binding, Blast, Bleeding, BlocksMovement,
    BloodCasting, Body, Chain, CooldownCasting, Dead, Deflect, Delay, Discovered, Duration,
    EffectTarget, Energy, Equippable, EquippedOn, Familiarity, Fizzle, Focus, Footwork, Found,
    Graze, Health, Hostile, Inventory, Item, Launcher, Learner, Light, Mana, ManaCasting, Melee,
    Name, OnHit, OnHitEntry, Player, Position, Proficiency, Projectile, Quiver, Recharging, Scroll,
    Skills, SpawnSeq, Speed, Spell, SpellAi, SpellDamage, Stats, Style, Summon, SummonLimit,
    Summoned, Throwable, Timed, Trains, Weapon,
};
use crate::entity::{Attuned, Lesson, Sighted, Studied};
use crate::explore::ExploreItems;
use crate::fov;
use crate::map::Grid;
use crate::rng::Rngs;
use crate::skill::{SkillBook, SkillId};
use crate::state::{GameState, MessageLog, event_registry};
use crate::style::StyleDef;
use crate::time::{Tick, Timer, Timers};

/// Bumped on any change to the format. Older saves are refused, not migrated.
pub const SAVE_VERSION: u32 = 28;

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
    UnknownSkillId(SkillId),
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
            Self::UnknownSkillId(id) => write!(f, "no skill with id {id:?}"),
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
    UnknownSkill(String),
    /// The save is in a dungeon the data no longer has.
    UnknownDungeon(String),
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
            Self::UnknownSkill(name) => write!(f, "save refers to unknown skill {name:?}"),
            Self::UnknownDungeon(name) => write!(f, "save is in unknown dungeon {name:?}"),
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
    place: crate::dive::Place,
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
    Ammo,
    Projectile,
    Launcher,
    Quiver,
    Spell,
    SpellDamage,
    Area,
    Recharging,
    CooldownCasting,
    ManaCasting,
    Mana,
    Duration,
    Timed,
    EffectTarget,
    Summoned,
    Summon,
    Binding,
    SummonLimit,
    BloodCasting,
    Scroll,
    Focus,
    Fizzle,
    Chain,
    Delay,
    Blast,
    SpellAi,
    Bleeding,
    OnHit,
    Skills,
    Trains,
    Learner,
    Familiarity,
    Graze,
    Deflect,
    Light,
    Proficiency,
    Footwork,
    Discovered,
    Style,
    Lesson,
    Studied,
    Sighted,
    Attuned,
}

impl ComponentKind {
    const ALL: [ComponentKind; 66] = [
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
        Self::Ammo,
        Self::Projectile,
        Self::Launcher,
        Self::Quiver,
        Self::Spell,
        Self::SpellDamage,
        Self::Area,
        Self::Recharging,
        Self::CooldownCasting,
        Self::ManaCasting,
        Self::Mana,
        Self::Duration,
        Self::Timed,
        Self::EffectTarget,
        Self::Summoned,
        Self::Summon,
        Self::Binding,
        Self::SummonLimit,
        Self::BloodCasting,
        Self::Scroll,
        Self::Focus,
        Self::Fizzle,
        Self::Chain,
        Self::Delay,
        Self::Blast,
        Self::SpellAi,
        Self::Bleeding,
        Self::OnHit,
        Self::Skills,
        Self::Trains,
        Self::Learner,
        Self::Familiarity,
        Self::Graze,
        Self::Deflect,
        Self::Light,
        Self::Proficiency,
        Self::Footwork,
        Self::Discovered,
        Self::Style,
        Self::Lesson,
        Self::Studied,
        Self::Sighted,
        Self::Attuned,
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
    Ammo(Ammo),
    Projectile(Projectile),
    Launcher(Launcher),
    Quiver(Quiver),
    Spell(Spell),
    SpellDamage(SpellDamage),
    Area(Area),
    Recharging(Recharging),
    CooldownCasting(CooldownCasting),
    ManaCasting(ManaCasting),
    Mana(Mana),
    Duration(Duration),
    Timed(Timed),
    EffectTarget(u32),
    Summoned(u32),
    Summon(SummonDef),
    Binding,
    SummonLimit(SummonLimit),
    BloodCasting(BloodCasting),
    Scroll,
    Focus(Focus),
    Fizzle(Fizzle),
    Chain(Chain),
    Delay(Delay),
    Blast(Blast),
    SpellAi,
    Bleeding(Bleeding),
    OnHit(Vec<OnHitDef>),
    /// XP by skill name.
    Skills(Vec<(String, u32)>),
    Trains(Vec<String>),
    Learner,
    /// Uses by skill name and kind of thing.
    Familiarity(Vec<(String, String, u32)>),
    Graze(Graze),
    Deflect(Deflect),
    Light(Light),
    Proficiency,
    Footwork(Footwork),
    Discovered(Discovered),
    /// Skills by name.
    Style(StyleDef),
    /// XP by skill name.
    Lesson(Vec<(String, u32)>),
    Studied(Studied),
    Sighted(Sighted),
    Attuned(Attuned),
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
        place: state.place.clone(),
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

/// The format version `text` was saved in, read without loading the rest,
/// or `None` if it is not a save.
pub fn version(text: &str) -> Option<u32> {
    ron::from_str::<Header>(text)
        .ok()
        .map(|header| header.version)
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
    let crate::dive::Place::Floor { dungeon, .. } = &save.place;
    if blueprints.dungeon(dungeon).is_none() {
        return Err(LoadError::UnknownDungeon(dungeon.clone()));
    }

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
        fx: Vec::new(),
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
        place: save.place,
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
        ComponentKind::Ammo => copy(entity).map(S::Ammo),
        ComponentKind::Projectile => copy(entity).map(S::Projectile),
        ComponentKind::Launcher => copy(entity).map(S::Launcher),
        ComponentKind::Quiver => copy(entity).map(S::Quiver),
        ComponentKind::Spell => copy(entity).map(S::Spell),
        ComponentKind::SpellDamage => copy(entity).map(S::SpellDamage),
        ComponentKind::Area => copy(entity).map(S::Area),
        ComponentKind::Recharging => copy(entity).map(S::Recharging),
        ComponentKind::CooldownCasting => copy(entity).map(S::CooldownCasting),
        ComponentKind::ManaCasting => copy(entity).map(S::ManaCasting),
        ComponentKind::Mana => copy(entity).map(S::Mana),
        ComponentKind::Duration => copy(entity).map(S::Duration),
        ComponentKind::Timed => copy(entity).map(S::Timed),
        ComponentKind::EffectTarget => match copy::<EffectTarget>(entity) {
            Some(EffectTarget(target)) => Some(S::EffectTarget(refer(target)?)),
            None => None,
        },
        ComponentKind::Summoned => match copy::<Summoned>(entity) {
            Some(summoned) => Some(S::Summoned(refer(summoned.owner)?)),
            None => None,
        },
        ComponentKind::Summon => match copy::<Summon>(entity) {
            Some(summon) => {
                let blueprint = blueprints
                    .by_id(summon.blueprint)
                    .ok_or(SaveError::UnknownBlueprintId(summon.blueprint))?;
                Some(S::Summon(SummonDef {
                    blueprint: blueprint.name.clone(),
                    count: summon.count,
                    lifetime: summon.lifetime,
                }))
            }
            None => None,
        },
        ComponentKind::Binding => copy::<Binding>(entity).map(|_| S::Binding),
        ComponentKind::SummonLimit => copy(entity).map(S::SummonLimit),
        ComponentKind::BloodCasting => copy(entity).map(S::BloodCasting),
        ComponentKind::Scroll => copy::<Scroll>(entity).map(|_| S::Scroll),
        ComponentKind::Focus => copy(entity).map(S::Focus),
        ComponentKind::Fizzle => copy(entity).map(S::Fizzle),
        ComponentKind::Chain => copy(entity).map(S::Chain),
        ComponentKind::Delay => copy(entity).map(S::Delay),
        ComponentKind::Blast => copy(entity).map(S::Blast),
        ComponentKind::SpellAi => copy::<SpellAi>(entity).map(|_| S::SpellAi),
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
                            on_graze: entry.on_graze,
                        })
                    })
                    .collect::<Result<_, SaveError>>()?,
            )),
            None => None,
        },
        ComponentKind::Skills => match copy::<Skills>(entity) {
            Some(Skills(entries)) => Some(S::Skills(
                entries
                    .into_iter()
                    .map(|(id, xp)| Ok((skill_name(blueprints.skills(), id)?, xp)))
                    .collect::<Result<_, SaveError>>()?,
            )),
            None => None,
        },
        ComponentKind::Trains => match copy::<Trains>(entity) {
            Some(Trains(ids)) => Some(S::Trains(
                ids.into_iter()
                    .map(|id| skill_name(blueprints.skills(), id))
                    .collect::<Result<_, SaveError>>()?,
            )),
            None => None,
        },
        ComponentKind::Learner => marker::<Learner>(entity, S::Learner),
        ComponentKind::Graze => copy(entity).map(S::Graze),
        ComponentKind::Deflect => copy(entity).map(S::Deflect),
        ComponentKind::Light => copy(entity).map(S::Light),
        ComponentKind::Proficiency => marker::<Proficiency>(entity, S::Proficiency),
        ComponentKind::Footwork => copy(entity).map(S::Footwork),
        ComponentKind::Discovered => copy(entity).map(S::Discovered),
        ComponentKind::Lesson => match copy::<Lesson>(entity) {
            Some(Lesson(grants)) => Some(S::Lesson(
                grants
                    .into_iter()
                    .map(|(id, xp)| Ok((skill_name(blueprints.skills(), id)?, xp)))
                    .collect::<Result<_, SaveError>>()?,
            )),
            None => None,
        },
        ComponentKind::Studied => copy(entity).map(S::Studied),
        ComponentKind::Sighted => copy(entity).map(S::Sighted),
        ComponentKind::Attuned => copy(entity).map(S::Attuned),
        ComponentKind::Style => match copy::<Style>(entity) {
            Some(style) => Some(S::Style(
                StyleDef::of(&style, blueprints.skills())
                    .ok_or(SaveError::UnknownSkillId(style.skill))?,
            )),
            None => None,
        },
        ComponentKind::Familiarity => match copy::<Familiarity>(entity) {
            Some(Familiarity(entries)) => Some(S::Familiarity(
                entries
                    .into_iter()
                    .map(|(id, kind, uses)| Ok((skill_name(blueprints.skills(), id)?, kind, uses)))
                    .collect::<Result<_, SaveError>>()?,
            )),
            None => None,
        },
    })
}

fn skill_name(book: &SkillBook, id: SkillId) -> Result<String, SaveError> {
    book.get(id)
        .map(|def| def.name.clone())
        .ok_or(SaveError::UnknownSkillId(id))
}

fn skill_id(book: &SkillBook, name: String) -> Result<SkillId, LoadError> {
    book.id(&name).ok_or(LoadError::UnknownSkill(name))
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
        S::Ammo(c) => builder.add(c),
        S::Projectile(c) => builder.add(c),
        S::Launcher(c) => builder.add(c),
        S::Quiver(c) => builder.add(c),
        S::Spell(c) => builder.add(c),
        S::SpellDamage(c) => builder.add(c),
        S::Area(c) => builder.add(c),
        S::Recharging(c) => builder.add(c),
        S::CooldownCasting(c) => builder.add(c),
        S::ManaCasting(c) => builder.add(c),
        S::Mana(c) => builder.add(c),
        S::Duration(c) => builder.add(c),
        S::Timed(c) => builder.add(c),
        S::EffectTarget(target) => builder.add(EffectTarget(entity(target)?)),
        S::Summoned(owner) => builder.add(Summoned {
            owner: entity(owner)?,
        }),
        S::Summon(def) => builder.add(Summon {
            blueprint: blueprints
                .id(&def.blueprint)
                .ok_or(LoadError::UnknownBlueprint(def.blueprint))?,
            count: def.count,
            lifetime: def.lifetime,
        }),
        S::Binding => builder.add(Binding),
        S::SummonLimit(c) => builder.add(c),
        S::BloodCasting(c) => builder.add(c),
        S::Scroll => builder.add(Scroll),
        S::Focus(c) => builder.add(c),
        S::Fizzle(c) => builder.add(c),
        S::Chain(c) => builder.add(c),
        S::Delay(c) => builder.add(c),
        S::Blast(c) => builder.add(c),
        S::SpellAi => builder.add(SpellAi),
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
                        on_graze: entry.on_graze,
                    })
                })
                .collect::<Result<_, LoadError>>()?,
        )),
        // Sorted by id, which follows the skill file: it may have been
        // reordered since the save.
        S::Skills(entries) => {
            let mut learned = Skills::default();
            for (name, xp) in entries {
                learned.set(skill_id(blueprints.skills(), name)?, xp);
            }
            builder.add(learned)
        }
        S::Trains(names) => {
            let mut ids = names
                .into_iter()
                .map(|name| skill_id(blueprints.skills(), name))
                .collect::<Result<Vec<_>, _>>()?;
            ids.sort();
            builder.add(Trains(ids))
        }
        S::Learner => builder.add(Learner),
        S::Graze(c) => builder.add(c),
        S::Deflect(c) => builder.add(c),
        S::Light(c) => builder.add(c),
        S::Proficiency => builder.add(Proficiency),
        S::Footwork(c) => builder.add(c),
        S::Discovered(c) => builder.add(c),
        S::Lesson(grants) => builder.add(Lesson(
            grants
                .into_iter()
                .map(|(name, xp)| Ok((skill_id(blueprints.skills(), name)?, xp)))
                .collect::<Result<_, LoadError>>()?,
        )),
        S::Studied(c) => builder.add(c),
        S::Sighted(c) => builder.add(c),
        S::Attuned(c) => builder.add(c),
        S::Style(def) => builder.add(
            def.resolve(blueprints.skills())
                .map_err(LoadError::UnknownSkill)?,
        ),
        S::Familiarity(entries) => {
            let mut familiarity = entries
                .into_iter()
                .map(|(name, kind, uses)| Ok((skill_id(blueprints.skills(), name)?, kind, uses)))
                .collect::<Result<Vec<_>, LoadError>>()?;
            familiarity.sort();
            builder.add(Familiarity(familiarity))
        }
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
    use crate::state::tests::{bare_state, test_blueprints};
    use crate::testkit::thing;
    use crate::time::{Step, step, take_turn};

    fn data_blueprints() -> Blueprints {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        Blueprints::load(&dir).unwrap()
    }

    fn new_game(seed: u64) -> GameState {
        GameState::new(seed, data_blueprints()).unwrap()
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
            goblins(&uninterrupted).len() < goblins(&new_game(42)).len(),
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
    fn a_quiver_its_arrows_and_a_nocked_arrow_survive_a_round_trip() {
        use crate::entity::{Ammo, AmmoKind, Launcher, Projectile, Quiver};
        let mut state = bare_state();
        let arrow = || {
            thing("arrow").item().ammo(AmmoKind::Arrow).projectile(
                1,
                3,
                &[DamageType::Piercing],
                2500,
            )
        };
        let bow = thing("bow")
            .item()
            .launcher(8, AmmoKind::Arrow, Stat::Agility)
            .equipped_on(state.player, 3)
            .make(&mut state);
        let quiver = thing("quiver")
            .item()
            .quiver(AmmoKind::Arrow, 10)
            .equipped_on(state.player, 2)
            .make(&mut state);
        for _ in 0..2 {
            let loaded = arrow().make(&mut state);
            crate::holding::stow(&mut state.world, quiver, loaded);
        }
        let nocked = arrow().make(&mut state);
        crate::holding::stow(&mut state.world, bow, nocked);

        let loaded = reload(&state, test_blueprints());
        let worn = crate::equipment::equipped(&loaded.world, loaded.player);
        let find = |has: fn(&hecs::World, hecs::Entity) -> bool| {
            worn.iter()
                .map(|&(item, _)| item)
                .find(|&item| has(&loaded.world, item))
        };
        let bow = find(|w, e| w.satisfies::<&Launcher>(e)).unwrap();
        let quiver = find(|w, e| w.satisfies::<&Quiver>(e)).unwrap();
        assert_eq!(crate::holding::carried(&loaded.world, quiver).len(), 2);
        let nocked = crate::holding::carried(&loaded.world, bow);
        assert_eq!(nocked.len(), 1);
        assert!(loaded.world.satisfies::<&Ammo>(nocked[0]));
        assert_eq!(
            loaded.world.get::<&Projectile>(nocked[0]).unwrap().break_bp,
            2500
        );
        assert_eq!(to_ron(&loaded).unwrap(), to_ron(&state).unwrap());
    }

    #[test]
    fn a_known_and_a_followed_style_survive_a_round_trip() {
        let mut state = bare_state();
        let player = state.player;
        let book = state.blueprints.skills();
        let style = Style {
            domains: vec![crate::skill::Domain::Martial],
            skill: book.id("Dagger").unwrap(),
            follows: vec![book.id("Melee").unwrap()],
            principles: vec![crate::entity::Principle {
                from_level: 5,
                does: crate::entity::Does::Riposte,
            }],
        };
        thing("knifework")
            .with(style.clone())
            .carried_by(player)
            .make(&mut state);
        thing("fencing")
            .with(style.clone())
            .attached_to(player)
            .make(&mut state);

        let loaded = reload(&state, test_blueprints());
        let known = crate::style::known(&loaded.world, loaded.player);
        assert_eq!(known.len(), 2);
        for thing in known {
            assert_eq!(*loaded.world.get::<&Style>(thing).unwrap(), style);
        }
        assert_eq!(
            crate::style::followed(&loaded.world, loaded.player).len(),
            1
        );
        assert_eq!(to_ron(&loaded).unwrap(), to_ron(&state).unwrap());
    }

    #[test]
    fn a_lesson_and_what_was_studied_survive_a_round_trip() {
        let mut state = bare_state();
        let player = state.player;
        let dagger = state.blueprints.skills().id("Dagger").unwrap();
        thing("manual")
            .item()
            .with(Lesson(vec![(dagger, 150)]))
            .carried_by(player)
            .make(&mut state);
        state
            .world
            .insert_one(player, Studied(vec!["primer".into()]))
            .unwrap();
        state
            .world
            .insert_one(player, Sighted(vec!["goblin".into()]))
            .unwrap();
        state
            .world
            .insert_one(player, Attuned(vec!["firebolt".into()]))
            .unwrap();

        let loaded = reload(&state, test_blueprints());
        let manual = crate::holding::carried(&loaded.world, loaded.player)[0];
        assert_eq!(
            *loaded.world.get::<&Lesson>(manual).unwrap(),
            Lesson(vec![(dagger, 150)])
        );
        assert_eq!(
            *loaded.world.get::<&Studied>(loaded.player).unwrap(),
            Studied(vec!["primer".into()])
        );
        assert_eq!(
            *loaded.world.get::<&Sighted>(loaded.player).unwrap(),
            Sighted(vec!["goblin".into()])
        );
        assert_eq!(
            *loaded.world.get::<&Attuned>(loaded.player).unwrap(),
            Attuned(vec!["firebolt".into()])
        );
    }

    #[test]
    fn discoveries_and_proficiencies_survive_a_round_trip() {
        let mut state = bare_state();
        let player = state.player;
        state
            .world
            .insert_one(player, Discovered(vec!["Footwork".into()]))
            .unwrap();
        thing("footwork")
            .with(Proficiency)
            .with(Footwork { perfect_bp: 500 })
            .attached_to(player)
            .make(&mut state);

        let loaded = reload(&state, test_blueprints());
        let player = loaded.player;
        assert_eq!(
            *loaded.world.get::<&Discovered>(player).unwrap(),
            Discovered(vec!["Footwork".into()])
        );
        let footwork = crate::holding::attached(&loaded.world, player)[0];
        assert!(loaded.world.satisfies::<&Proficiency>(footwork));
        assert_eq!(
            *loaded.world.get::<&Footwork>(footwork).unwrap(),
            Footwork { perfect_bp: 500 }
        );
    }

    #[test]
    fn graze_and_deflect_chances_survive_a_round_trip() {
        let mut state = bare_state();
        let player = state.player;
        thing("rapier")
            .weapon(1, 6, Stat::Agility)
            .with(Graze(4_000))
            .equipped_on(player, 2)
            .make(&mut state);
        thing("buckler")
            .with(Deflect(2_000))
            .equipped_on(player, 3)
            .make(&mut state);

        let loaded = reload(&state, test_blueprints());
        let worn = crate::equipment::equipped(&loaded.world, loaded.player);
        let has = |part| worn.iter().find(|&&(_, on)| on == part).unwrap().0;
        assert_eq!(*loaded.world.get::<&Graze>(has(2)).unwrap(), Graze(4_000));
        assert_eq!(
            *loaded.world.get::<&Deflect>(has(3)).unwrap(),
            Deflect(2_000)
        );
    }

    #[test]
    fn skills_familiarity_and_what_a_weapon_trains_survive_a_round_trip() {
        let mut state = bare_state();
        let player = state.player;
        let book = state.blueprints.skills();
        let (melee, dagger) = (book.id("Melee").unwrap(), book.id("Dagger").unwrap());
        state.world.insert_one(player, Learner).unwrap();
        let goblin = thing("goblin")
            .health(50)
            .hostile()
            .at(2, 1)
            .make(&mut state);
        thing("knife")
            .weapon(1, 3, Stat::Agility)
            .with(Trains(vec![dagger]))
            .equipped_on(player, 2)
            .make(&mut state);
        state.run(Command::Attack {
            attacker: player,
            target: goblin,
        });
        let skills = (*state.world.get::<&Skills>(player).unwrap()).clone();
        let familiarity = (*state.world.get::<&Familiarity>(player).unwrap()).clone();
        assert!(skills.xp(melee) > 0 && skills.xp(dagger) > 0);

        let loaded = reload(&state, test_blueprints());
        let player = loaded.player;
        assert!(loaded.world.satisfies::<&Learner>(player));
        assert_eq!(*loaded.world.get::<&Skills>(player).unwrap(), skills);
        assert_eq!(
            *loaded.world.get::<&Familiarity>(player).unwrap(),
            familiarity
        );
        let (knife, _) = crate::equipment::equipped(&loaded.world, player)[0];
        assert_eq!(
            *loaded.world.get::<&Trains>(knife).unwrap(),
            Trains(vec![dagger])
        );
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
    fn a_recharging_spell_and_its_discipline_survive_a_round_trip() {
        let mut state = bare_state();
        let player = state.player;
        let _ = state.world.insert_one(
            player,
            CooldownCasting {
                school: crate::entity::School::Pyromancy,
                ticks_per_power: 300,
                stat: crate::entity::Stat::Willpower,
            },
        );
        let spell = thing("firebolt")
            .spell(crate::entity::School::Pyromancy, 6, 2)
            .spell_damage(3, 3)
            .attached_to(player)
            .make(&mut state);
        crate::spell::recharge(&mut state, spell, 600);

        let loaded = reload(&state, test_blueprints());
        let known = crate::view::known_spells(&loaded);
        assert_eq!(known.len(), 1);
        let recharging = *loaded.world.get::<&Recharging>(known[0]).unwrap();
        assert_eq!(recharging.0.duration, 600);
        assert_eq!(
            crate::spell::check_cast(&loaded, loaded.player, known[0], (3, 1)),
            Err("Your firebolt is not ready.".to_string())
        );
    }

    #[test]
    fn a_summon_its_binding_and_the_spell_that_called_it_survive_a_round_trip() {
        let mut state = bare_state();
        let player = state.player;
        let wolf_id = state.blueprints.id("SpiritWolf").unwrap();
        let summon = Summon {
            blueprint: wolf_id,
            count: 1,
            lifetime: 3000,
        };
        thing("summon wolf")
            .spell(crate::entity::School::Arcane, 5, 3)
            .with(summon)
            .attached_to(player)
            .make(&mut state);
        let _ = state.world.insert_one(player, SummonLimit { max: 2 });
        let wolf = thing("spirit wolf")
            .at(2, 1)
            .health(8)
            .summoned_by(player)
            .make(&mut state);
        crate::effect::bind(&mut state, wolf, 3000);

        let loaded = reload(&state, test_blueprints());
        let spell = crate::view::known_spells(&loaded)[0];
        let summon_after = *loaded.world.get::<&Summon>(spell).unwrap();
        assert_eq!(
            summon_after.blueprint,
            loaded.blueprints.id("SpiritWolf").unwrap()
        );
        assert_eq!((summon_after.count, summon_after.lifetime), (1, 3000));
        let (wolf, summoned) = loaded
            .world
            .query::<(Entity, &Summoned)>()
            .iter()
            .map(|(entity, summoned)| (entity, *summoned))
            .next()
            .unwrap();
        assert_eq!(summoned.owner, loaded.player);
        let bound = crate::holding::attached(&loaded.world, wolf)
            .into_iter()
            .any(|effect| loaded.world.satisfies::<(&Binding, &Timed)>(effect));
        assert!(bound);
        assert_eq!(
            loaded.world.get::<&SummonLimit>(loaded.player).unwrap().max,
            2
        );
    }

    #[test]
    fn a_spells_area_survives_a_round_trip() {
        let mut state = bare_state();
        let player = state.player;
        let spell = thing("fireball")
            .spell(crate::entity::School::Pyromancy, 6, 4)
            .spell_damage(3, 6)
            .area(
                crate::entity::Shape::Burst { radius: 1 },
                crate::entity::Hits::All,
                1,
            )
            .attached_to(player)
            .make(&mut state);
        let before = *state.world.get::<&Area>(spell).unwrap();

        let loaded = reload(&state, test_blueprints());
        let known = crate::view::known_spells(&loaded);
        assert_eq!(*loaded.world.get::<&Area>(known[0]).unwrap(), before);
    }

    #[test]
    fn a_part_spent_mana_pool_and_its_discipline_survive_a_round_trip() {
        let mut state = bare_state();
        let player = state.player;
        let rule = ManaCasting {
            school: crate::entity::School::Arcane,
            mana_per_power: 3,
            stat: crate::entity::Stat::Intelligence,
        };
        let pool = Mana {
            value: 10,
            max: 10,
            as_of: Tick::default(),
            regen_ticks: 200,
            growth: vec![crate::entity::ManaGrowth {
                stat: crate::entity::Stat::Willpower,
                max: 1,
                regen_bp: 2_000,
            }],
        };
        let _ = state.world.insert(player, (rule, pool));
        state.now = state.now.after(1_000);
        crate::spell::spend_mana(&mut state, player, 4);
        state.now = state.now.after(250);

        let loaded = reload(&state, test_blueprints());
        let (mana, rates) = crate::spell::pool(&loaded.world, loaded.player).unwrap();
        assert_eq!(
            *loaded.world.get::<&ManaCasting>(loaded.player).unwrap(),
            rule
        );
        assert_eq!(mana, *state.world.get::<&Mana>(player).unwrap());
        assert_eq!(mana.growth.len(), 1);
        assert_eq!(mana.current(rates, loaded.now), 7);
    }

    #[test]
    fn found_and_unfound_floor_items_survive_a_round_trip() {
        let mut state = bare_state();
        thing("found").item().found().at(4, 4).make(&mut state);
        thing("unfound").item().at(5, 4).make(&mut state);
        state.map.set(6, 4, Tile::StairsDown);

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
        assert_eq!(loaded.map.get(6, 4), Some(&Tile::StairsDown));
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
