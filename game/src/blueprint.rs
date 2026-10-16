//! Data-driven entity definitions, loaded from `data/entities/*.ron`. A
//! blueprint's name is its file stem. Inheritance is resolved once at load
//! time, so spawning never walks the parent chain.
//!
//! Inheritance follows one rule: the child's field wins. Each component
//! variant counts as its own field (a child's `Health` replaces the parent's,
//! other parent components are kept), and `equipment` is a single field
//! (absent inherits the parent's list, present replaces it, `[]` clears it).
//!
//! Every name a blueprint refers to (`inherits`, `equipment`, on-hit effects)
//! is checked and turned into a `BlueprintId` at load, so a typo fails
//! loading instead of surfacing mid-game.

use std::collections::BTreeMap;
use std::fs;
use std::mem::discriminant;
use std::path::{Path, PathBuf};

use hecs::{Entity, EntityBuilder, World};
use ron::extensions::Extensions;
use serde::{Deserialize, Serialize};

use crate::entity::{
    Ammo, Appearance, Armor, Bleeding, BlocksMovement, Body, BodyPart, CooldownCasting, Duration,
    Equippable, Health, Hostile, Item, Launcher, Mana, ManaCasting, Melee, Name, OnHit, OnHitEntry,
    PartKind, Player, Projectile, Quiver, School, Speed, Spell, SpellDamage, Stats, Throwable,
    Weapon,
};
use crate::equipment::free_part;
use crate::holding;

/// A blueprint's index in the `Blueprints` it was loaded into. Runtime-only:
/// ids depend on which files were loaded, so it has no serde derives and
/// anything persisted stores the blueprint's name instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlueprintId(u32);

/// Every component a blueprint can declare. A child's entry overrides the
/// parent's entry of the same variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ComponentDef {
    Name(Name),
    Appearance(Appearance),
    BlocksMovement,
    Player,
    Health(Health),
    Armor(Armor),
    Melee(Melee),
    Speed(Speed),
    Hostile,
    Item,
    Body(Vec<BodyPart>),
    Equippable(Equippable),
    Stats(Stats),
    Weapon(Weapon),
    Throwable(Throwable),
    Ammo(Ammo),
    Projectile(Projectile),
    Launcher(Launcher),
    Quiver(Quiver),
    Spell(Spell),
    SpellDamage(SpellDamage),
    CooldownCasting(CooldownCasting),
    ManaCasting(ManaCasting),
    Mana(Mana),
    Bleeding(Bleeding),
    Duration(Duration),
    OnHit(Vec<OnHitDef>),
}

/// An `OnHitEntry` as written in RON: the effect by blueprint name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnHitDef {
    pub effect: String,
    pub chance_bp: u32,
}

impl ComponentDef {
    fn add_to(&self, builder: &mut EntityBuilder) {
        match self.clone() {
            ComponentDef::Name(c) => builder.add(c),
            ComponentDef::Appearance(c) => builder.add(c),
            ComponentDef::BlocksMovement => builder.add(BlocksMovement),
            ComponentDef::Player => builder.add(Player),
            ComponentDef::Health(c) => builder.add(c),
            ComponentDef::Armor(c) => builder.add(c),
            ComponentDef::Melee(c) => builder.add(c),
            ComponentDef::Speed(c) => builder.add(c),
            ComponentDef::Hostile => builder.add(Hostile),
            ComponentDef::Item => builder.add(Item),
            ComponentDef::Body(parts) => builder.add(Body(parts)),
            ComponentDef::Equippable(c) => builder.add(c),
            ComponentDef::Stats(c) => builder.add(c),
            ComponentDef::Weapon(c) => builder.add(c),
            ComponentDef::Throwable(c) => builder.add(c),
            ComponentDef::Ammo(c) => builder.add(c),
            ComponentDef::Projectile(c) => builder.add(c),
            ComponentDef::Launcher(c) => builder.add(c),
            ComponentDef::Spell(c) => builder.add(c),
            ComponentDef::SpellDamage(c) => builder.add(c),
            ComponentDef::CooldownCasting(c) => builder.add(c),
            ComponentDef::ManaCasting(c) => builder.add(c),
            ComponentDef::Mana(c) => builder.add(c),
            ComponentDef::Quiver(c) => builder.add(c),
            ComponentDef::Bleeding(c) => builder.add(c),
            ComponentDef::Duration(c) => builder.add(c),
            // Holds names; spawned from `ResolvedBlueprint::on_hit` instead.
            ComponentDef::OnHit(_) => return,
        };
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Blueprint {
    pub inherits: Option<String>,
    pub components: Vec<ComponentDef>,
    /// Blueprint names spawned as separate entities and attached to this one.
    /// `None` inherits the parent's list; `Some` (even empty) replaces it.
    pub equipment: Option<Vec<String>>,
}

impl Blueprint {
    /// The equipment list, treating `None` as empty.
    pub fn equipment(&self) -> &[String] {
        self.equipment.as_deref().unwrap_or_default()
    }
}

/// A blueprint after load: inheritance flattened and every reference an id.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedBlueprint {
    pub name: String,
    /// Every declared component except `OnHit`, which lives in `on_hit`.
    pub components: Vec<ComponentDef>,
    pub on_hit: Option<OnHit>,
    pub equipment: Vec<BlueprintId>,
    /// The `Body` part each `equipment` entry is spawned on, index for index.
    pub equipped_on: Vec<u32>,
}

impl ResolvedBlueprint {
    pub fn body(&self) -> &[BodyPart] {
        self.components
            .iter()
            .find_map(|c| match c {
                ComponentDef::Body(parts) => Some(parts.as_slice()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// How long an effect made from this lasts, in ticks.
    pub fn duration(&self) -> Option<u64> {
        self.components.iter().find_map(|c| match c {
            ComponentDef::Duration(Duration(ticks)) => Some(*ticks),
            _ => None,
        })
    }

    pub fn equippable(&self) -> Option<PartKind> {
        self.components.iter().find_map(|c| match c {
            ComponentDef::Equippable(e) => Some(e.part),
            _ => None,
        })
    }
}

/// A loaded, validated set of blueprints.
#[derive(Debug, Clone, Default)]
pub struct Blueprints {
    /// Indexed by `BlueprintId`; ids follow name order.
    resolved: Vec<ResolvedBlueprint>,
    ids: BTreeMap<String, BlueprintId>,
}

#[derive(Debug)]
pub enum BlueprintError {
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, Box<ron::error::SpannedError>),
    UnknownBlueprint(String),
    UnknownParent {
        child: String,
        parent: String,
    },
    UnknownEquipment {
        owner: String,
        item: String,
    },
    UnknownEffect {
        owner: String,
        effect: String,
    },
    EffectWithoutDuration {
        owner: String,
        effect: String,
    },
    /// Names along the cycle, starting and ending with the same blueprint.
    InheritanceCycle(Vec<String>),
    EquipmentCycle(Vec<String>),
    /// `item` can't be equipped, or `owner`'s `Body` has no free part for it.
    EquipmentDoesNotFit {
        owner: String,
        item: String,
    },
    /// Two of `owner`'s disciplines claim `school`; one must (ADR 0014).
    SchoolClaimedTwice {
        owner: String,
        school: School,
    },
}

impl std::fmt::Display for BlueprintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(path, err) => write!(f, "{}: {err}", path.display()),
            Self::Parse(path, err) => write!(f, "{}:{err}", path.display()),
            Self::UnknownBlueprint(name) => write!(f, "unknown blueprint {name:?}"),
            Self::UnknownParent { child, parent } => {
                write!(f, "blueprint {child:?} inherits from unknown {parent:?}")
            }
            Self::UnknownEquipment { owner, item } => {
                write!(f, "blueprint {owner:?} is equipped with unknown {item:?}")
            }
            Self::UnknownEffect { owner, effect } => {
                write!(
                    f,
                    "blueprint {owner:?} applies unknown effect {effect:?} on hit"
                )
            }
            Self::EffectWithoutDuration { owner, effect } => write!(
                f,
                "blueprint {owner:?} applies effect {effect:?} on hit, which has no Duration"
            ),
            Self::InheritanceCycle(path) => write!(f, "inheritance cycle: {}", path.join(" -> ")),
            Self::EquipmentCycle(path) => write!(f, "equipment cycle: {}", path.join(" -> ")),
            Self::EquipmentDoesNotFit { owner, item } => {
                write!(
                    f,
                    "blueprint {owner:?} has no free body part for equipment {item:?}"
                )
            }
            Self::SchoolClaimedTwice { owner, school } => {
                write!(f, "blueprint {owner:?} has two disciplines for {school:?}")
            }
        }
    }
}

impl std::error::Error for BlueprintError {}

fn ron_options() -> ron::Options {
    ron::Options::default()
        .with_default_extension(Extensions::UNWRAP_VARIANT_NEWTYPES | Extensions::IMPLICIT_SOME)
}

impl Blueprints {
    /// Loads every `*.ron` file in `dir`, named by file stem, and resolves them.
    pub fn load_dir(dir: &Path) -> Result<Self, BlueprintError> {
        let io_err = |err| BlueprintError::Io(dir.to_path_buf(), err);
        let mut raw = BTreeMap::new();
        for entry in fs::read_dir(dir).map_err(io_err)? {
            let path = entry.map_err(io_err)?.path();
            if path.extension().is_none_or(|ext| ext != "ron") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let name = name.to_string();
            let contents =
                fs::read_to_string(&path).map_err(|err| BlueprintError::Io(path.clone(), err))?;
            let blueprint = ron_options()
                .from_str(&contents)
                .map_err(|err| BlueprintError::Parse(path.clone(), Box::new(err)))?;
            raw.insert(name, blueprint);
        }
        Self::resolve(raw)
    }

    /// Checks that every referenced name exists, flattens inheritance,
    /// assigns ids in name order, and rejects on-hit effects without a
    /// `Duration` and equipment cycles, so neither can fail during play.
    pub fn resolve(raw: BTreeMap<String, Blueprint>) -> Result<Self, BlueprintError> {
        check_references(&raw)?;
        let mut flat = BTreeMap::new();
        for name in raw.keys() {
            resolve_one(name, &raw, &mut flat, &mut Vec::new())?;
        }
        check_effect_durations(&raw, &flat)?;
        check_disciplines(&flat)?;

        let ids: BTreeMap<String, BlueprintId> =
            flat.keys().cloned().zip((0..).map(BlueprintId)).collect();
        let id_of = |name: &String| {
            ids.get(name)
                .copied()
                .ok_or_else(|| BlueprintError::UnknownBlueprint(name.clone()))
        };
        let mut resolved = Vec::with_capacity(flat.len());
        for (name, blueprint) in flat {
            let mut components = Vec::with_capacity(blueprint.components.len());
            let mut on_hit = None;
            for component in blueprint.components {
                match component {
                    ComponentDef::OnHit(entries) => {
                        let entries = entries
                            .iter()
                            .map(|entry| {
                                Ok(OnHitEntry {
                                    effect: id_of(&entry.effect)?,
                                    chance_bp: entry.chance_bp,
                                })
                            })
                            .collect::<Result<_, BlueprintError>>()?;
                        on_hit = Some(OnHit(entries));
                    }
                    other => components.push(other),
                }
            }
            let equipment = blueprint
                .equipment
                .iter()
                .flatten()
                .map(id_of)
                .collect::<Result<_, _>>()?;
            resolved.push(ResolvedBlueprint {
                name,
                components,
                on_hit,
                equipment,
                equipped_on: Vec::new(),
            });
        }
        check_equipment_cycles(&resolved)?;
        fit_equipment(&mut resolved)?;
        Ok(Self { resolved, ids })
    }

    pub fn id(&self, name: &str) -> Option<BlueprintId> {
        self.ids.get(name).copied()
    }

    pub fn by_id(&self, id: BlueprintId) -> Option<&ResolvedBlueprint> {
        self.resolved.get(id.0 as usize)
    }

    pub fn get(&self, name: &str) -> Option<&ResolvedBlueprint> {
        self.by_id(self.id(name)?)
    }

    /// Every blueprint with an `Item` component, in id (name) order so
    /// picking from it by index is deterministic.
    pub fn items(&self) -> Vec<BlueprintId> {
        (0..)
            .zip(&self.resolved)
            .filter(|(_, blueprint)| blueprint.components.contains(&ComponentDef::Item))
            .map(|(id, _)| BlueprintId(id))
            .collect()
    }

    /// Spawns blueprint `id`, and each piece of its equipment as a separate
    /// entity put on the owner's body part (`holding::put_on`). Only for
    /// `GameState::spawn_id`, which gives them all a `SpawnSeq`. Equipment
    /// cycles are rejected at load, so the recursion ends.
    #[expect(
        clippy::disallowed_methods,
        reason = "the blueprint half of GameState::spawn_id, which registers what this creates"
    )]
    pub(crate) fn spawn_id(
        &self,
        world: &mut World,
        id: BlueprintId,
    ) -> Result<Entity, BlueprintError> {
        let blueprint = self
            .by_id(id)
            .ok_or_else(|| BlueprintError::UnknownBlueprint(format!("{id:?}")))?;

        let mut builder = EntityBuilder::new();
        for component in &blueprint.components {
            component.add_to(&mut builder);
        }
        if let Some(on_hit) = &blueprint.on_hit {
            builder.add(on_hit.clone());
        }
        let owner = world.spawn(builder.build());
        for (&item, &part) in blueprint.equipment.iter().zip(&blueprint.equipped_on) {
            let item = self.spawn_id(world, item)?;
            holding::put_on(world, owner, item, part);
        }
        Ok(owner)
    }
}

/// On-hit effects are applied with `ApplyEffect`, which needs a `Duration`
/// (possibly inherited, hence checked on `flat`). Reported against the
/// blueprint that wrote the entry, like `check_references`.
fn check_effect_durations(
    raw: &BTreeMap<String, Blueprint>,
    flat: &BTreeMap<String, Blueprint>,
) -> Result<(), BlueprintError> {
    for (owner, blueprint) in raw {
        let effects = blueprint
            .components
            .iter()
            .flat_map(|component| match component {
                ComponentDef::OnHit(entries) => entries.as_slice(),
                _ => &[],
            });
        for entry in effects {
            let has_duration = flat.get(&entry.effect).is_some_and(|effect| {
                effect
                    .components
                    .iter()
                    .any(|c| matches!(c, ComponentDef::Duration(_)))
            });
            if !has_duration {
                return Err(BlueprintError::EffectWithoutDuration {
                    owner: owner.clone(),
                    effect: entry.effect.clone(),
                });
            }
        }
    }
    Ok(())
}

/// A school is claimed by one discipline per caster, so which one pays for
/// a cast never depends on handler order. Checked after flattening: a child
/// can add a discipline to an inherited one.
fn check_disciplines(flat: &BTreeMap<String, Blueprint>) -> Result<(), BlueprintError> {
    for (owner, blueprint) in flat {
        let mut schools = Vec::new();
        for component in &blueprint.components {
            let school = match component {
                ComponentDef::CooldownCasting(rule) => rule.school,
                ComponentDef::ManaCasting(rule) => rule.school,
                _ => continue,
            };
            if schools.contains(&school) {
                return Err(BlueprintError::SchoolClaimedTwice {
                    owner: owner.clone(),
                    school,
                });
            }
            schools.push(school);
        }
    }
    Ok(())
}

/// Checked after flattening, since a cycle can run through inherited
/// equipment lists.
/// Puts each blueprint's equipment on the first free body part that fits,
/// in list order, so spawning can never fail to place it.
fn fit_equipment(resolved: &mut [ResolvedBlueprint]) -> Result<(), BlueprintError> {
    for owner in 0..resolved.len() {
        let mut parts = Vec::with_capacity(resolved[owner].equipment.len());
        for &item in &resolved[owner].equipment {
            let item = &resolved[item.0 as usize];
            let part = item
                .equippable()
                .and_then(|kind| free_part(resolved[owner].body(), &parts, kind))
                .ok_or_else(|| BlueprintError::EquipmentDoesNotFit {
                    owner: resolved[owner].name.clone(),
                    item: item.name.clone(),
                })?;
            parts.push(part);
        }
        resolved[owner].equipped_on = parts;
    }
    Ok(())
}

fn check_equipment_cycles(resolved: &[ResolvedBlueprint]) -> Result<(), BlueprintError> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Open,
        Done,
    }

    fn visit(
        resolved: &[ResolvedBlueprint],
        id: usize,
        marks: &mut [Mark],
        path: &mut Vec<usize>,
    ) -> Result<(), BlueprintError> {
        match marks[id] {
            Mark::Done => return Ok(()),
            Mark::Open => {
                let start = path.iter().position(|&p| p == id).unwrap_or_default();
                let cycle = path[start..]
                    .iter()
                    .chain([&id])
                    .map(|&i| resolved[i].name.clone())
                    .collect();
                return Err(BlueprintError::EquipmentCycle(cycle));
            }
            Mark::New => {}
        }
        marks[id] = Mark::Open;
        path.push(id);
        for item in &resolved[id].equipment {
            visit(resolved, item.0 as usize, marks, path)?;
        }
        path.pop();
        marks[id] = Mark::Done;
        Ok(())
    }

    let mut marks = vec![Mark::New; resolved.len()];
    for id in 0..resolved.len() {
        visit(resolved, id, &mut marks, &mut Vec::new())?;
    }
    Ok(())
}

/// Checks equipment and on-hit names as written, before flattening, so the
/// error names the blueprint that wrote the bad reference rather than a
/// child that inherited it. `resolve_one` checks `inherits`.
fn check_references(raw: &BTreeMap<String, Blueprint>) -> Result<(), BlueprintError> {
    for (owner, blueprint) in raw {
        if let Some(item) = blueprint
            .equipment()
            .iter()
            .find(|item| !raw.contains_key(*item))
        {
            return Err(BlueprintError::UnknownEquipment {
                owner: owner.clone(),
                item: item.clone(),
            });
        }
        let effects = blueprint
            .components
            .iter()
            .flat_map(|component| match component {
                ComponentDef::OnHit(entries) => entries.as_slice(),
                _ => &[],
            });
        for entry in effects {
            if !raw.contains_key(&entry.effect) {
                return Err(BlueprintError::UnknownEffect {
                    owner: owner.clone(),
                    effect: entry.effect.clone(),
                });
            }
        }
    }
    Ok(())
}

fn resolve_one(
    name: &str,
    raw: &BTreeMap<String, Blueprint>,
    resolved: &mut BTreeMap<String, Blueprint>,
    stack: &mut Vec<String>,
) -> Result<(), BlueprintError> {
    if resolved.contains_key(name) {
        return Ok(());
    }
    if let Some(start) = stack.iter().position(|n| n == name) {
        let mut cycle = stack[start..].to_vec();
        cycle.push(name.to_string());
        return Err(BlueprintError::InheritanceCycle(cycle));
    }
    let blueprint = &raw[name];

    let mut flat = Blueprint::default();
    if let Some(parent) = &blueprint.inherits {
        if !raw.contains_key(parent) {
            return Err(BlueprintError::UnknownParent {
                child: name.to_string(),
                parent: parent.clone(),
            });
        }
        stack.push(name.to_string());
        resolve_one(parent, raw, resolved, stack)?;
        stack.pop();
        flat = resolved[parent].clone();
    }

    for component in &blueprint.components {
        match flat
            .components
            .iter_mut()
            .find(|existing| discriminant(*existing) == discriminant(component))
        {
            Some(slot) => *slot = component.clone(),
            None => flat.components.push(component.clone()),
        }
    }
    if blueprint.equipment.is_some() {
        flat.equipment = blueprint.equipment.clone();
    }

    resolved.insert(name.to_string(), flat);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/entities")
    }

    fn bp(inherits: Option<&str>, components: Vec<ComponentDef>) -> Blueprint {
        Blueprint {
            inherits: inherits.map(str::to_string),
            components,
            equipment: None,
        }
    }

    fn with_equipment(mut blueprint: Blueprint, equipment: Option<&[&str]>) -> Blueprint {
        blueprint.equipment = equipment.map(|items| items.iter().map(|s| s.to_string()).collect());
        blueprint
    }

    /// `Base` carries a sword; `Child` sets `equipment` to `child_equipment`.
    fn held() -> ComponentDef {
        ComponentDef::Equippable(Equippable {
            part: PartKind::Hand,
        })
    }

    fn two_hands() -> ComponentDef {
        ComponentDef::Body(
            ["right hand", "left hand"]
                .map(|name| BodyPart {
                    kind: PartKind::Hand,
                    name: name.to_string(),
                })
                .to_vec(),
        )
    }

    fn resolve_child_equipment(child_equipment: Option<&[&str]>) -> Vec<String> {
        let raw = BTreeMap::from([
            ("Sword".to_string(), bp(None, vec![held()])),
            ("Shield".to_string(), bp(None, vec![held()])),
            (
                "Base".to_string(),
                with_equipment(bp(None, vec![two_hands()]), Some(&["Sword"])),
            ),
            (
                "Child".to_string(),
                with_equipment(bp(Some("Base"), vec![]), child_equipment),
            ),
        ]);
        let blueprints = Blueprints::resolve(raw).unwrap();
        blueprints
            .get("Child")
            .unwrap()
            .equipment
            .iter()
            .map(|&id| blueprints.by_id(id).unwrap().name.clone())
            .collect()
    }

    #[test]
    fn equipment_that_does_not_fit_fails_the_load() {
        let fits = |owner: Vec<ComponentDef>, item: Vec<ComponentDef>, gear: &[&str]| {
            let raw = BTreeMap::from([
                ("Sword".to_string(), bp(None, item)),
                (
                    "Owner".to_string(),
                    with_equipment(bp(None, owner), Some(gear)),
                ),
            ]);
            match Blueprints::resolve(raw) {
                Ok(_) => Ok(()),
                Err(BlueprintError::EquipmentDoesNotFit { owner, item }) => Err((owner, item)),
                Err(other) => panic!("unexpected {other}"),
            }
        };
        let refused = Err(("Owner".to_string(), "Sword".to_string()));
        assert_eq!(
            fits(vec![two_hands()], vec![held()], &["Sword", "Sword"]),
            Ok(())
        );
        assert_eq!(
            fits(vec![two_hands()], vec![held()], &["Sword"; 3]),
            refused
        );
        assert_eq!(fits(vec![two_hands()], vec![], &["Sword"]), refused);
        assert_eq!(fits(vec![], vec![held()], &["Sword"]), refused);
    }

    #[test]
    fn two_disciplines_for_one_school_fail_the_load() {
        use crate::entity::Stat;
        let cooldown = |school| {
            ComponentDef::CooldownCasting(CooldownCasting {
                school,
                ticks_per_power: 100,
                stat: Stat::Willpower,
            })
        };
        let mana = |school| {
            ComponentDef::ManaCasting(ManaCasting {
                school,
                mana_per_power: 1,
                stat: Stat::Intelligence,
            })
        };
        let load = |child: ComponentDef| {
            Blueprints::resolve(BTreeMap::from([
                (
                    "Base".to_string(),
                    bp(None, vec![cooldown(School::Pyromancy)]),
                ),
                ("Mage".to_string(), bp(Some("Base"), vec![child])),
            ]))
        };
        assert!(load(mana(School::Arcane)).is_ok());
        match load(mana(School::Pyromancy)) {
            Err(BlueprintError::SchoolClaimedTwice { owner, school }) => {
                assert_eq!((owner.as_str(), school), ("Mage", School::Pyromancy));
            }
            other => panic!("expected SchoolClaimedTwice, got {other:?}"),
        }
    }

    #[test]
    fn child_without_equipment_inherits_the_parents() {
        assert_eq!(resolve_child_equipment(None), ["Sword"]);
    }

    #[test]
    fn child_equipment_replaces_the_parents() {
        assert_eq!(resolve_child_equipment(Some(&["Shield"])), ["Shield"]);
    }

    #[test]
    fn empty_child_equipment_clears_the_parents() {
        assert!(resolve_child_equipment(Some(&[])).is_empty());
    }

    #[test]
    fn equipment_parses_absent_present_and_empty() {
        let parse = |src: &str| ron_options().from_str::<Blueprint>(src).unwrap().equipment;
        assert_eq!(parse("(components: [])"), None);
        assert_eq!(
            parse(r#"(equipment: ["Sword"])"#),
            Some(vec!["Sword".to_string()])
        );
        assert_eq!(parse("(equipment: [])"), Some(vec![]));
    }

    #[test]
    fn loads_the_data_directory() {
        let blueprints = Blueprints::load_dir(&data_dir()).expect("data should load");
        for name in [
            "Bleeding",
            "Creature",
            "Goblin",
            "LeatherArmor",
            "Player",
            "RustyKnife",
        ] {
            assert!(blueprints.get(name).is_some(), "missing {name}");
        }
        let id = |name| blueprints.id(name).unwrap();
        assert_eq!(
            blueprints.get("Goblin").unwrap().equipment,
            [id("LeatherArmor"), id("RustyKnife")]
        );
        assert_eq!(
            blueprints.get("RustyKnife").unwrap().on_hit,
            Some(OnHit(vec![OnHitEntry {
                effect: id("Bleeding"),
                chance_bp: 2500,
            }]))
        );
        assert!(
            blueprints
                .get("Player")
                .unwrap()
                .components
                .contains(&ComponentDef::Player)
        );
        let goblin = &blueprints.get("Goblin").unwrap().components;
        assert!(goblin.contains(&ComponentDef::Hostile));
        assert!(goblin.contains(&ComponentDef::Speed(Speed(100))));
        let bleeding = &blueprints.get("Bleeding").unwrap().components;
        assert!(bleeding.contains(&ComponentDef::Duration(Duration(500))));
    }

    #[test]
    fn child_components_override_the_parents_of_the_same_variant() {
        let raw = BTreeMap::from([
            (
                "Base".to_string(),
                bp(
                    None,
                    vec![
                        ComponentDef::BlocksMovement,
                        ComponentDef::Health(Health {
                            current: 10,
                            max: 10,
                        }),
                    ],
                ),
            ),
            (
                "Child".to_string(),
                bp(
                    Some("Base"),
                    vec![
                        ComponentDef::Health(Health { current: 3, max: 3 }),
                        ComponentDef::Melee(Melee { min: 1, max: 2 }),
                    ],
                ),
            ),
        ]);
        let blueprints = Blueprints::resolve(raw).unwrap();
        let child = blueprints.get("Child").unwrap();
        assert_eq!(
            child.components,
            [
                ComponentDef::BlocksMovement,
                ComponentDef::Health(Health { current: 3, max: 3 }),
                ComponentDef::Melee(Melee { min: 1, max: 2 }),
            ]
        );
    }

    #[test]
    fn inheritance_cycles_are_reported() {
        let raw = BTreeMap::from([
            ("A".to_string(), bp(Some("B"), vec![])),
            ("B".to_string(), bp(Some("C"), vec![])),
            ("C".to_string(), bp(Some("A"), vec![])),
        ]);
        match Blueprints::resolve(raw) {
            Err(BlueprintError::InheritanceCycle(path)) => assert_eq!(path, ["A", "B", "C", "A"]),
            other => panic!("expected a cycle error, got {other:?}"),
        }
    }

    fn load_error(raw: Vec<(&str, Blueprint)>) -> String {
        let raw = raw
            .into_iter()
            .map(|(name, blueprint)| (name.to_string(), blueprint))
            .collect();
        match Blueprints::resolve(raw) {
            Err(err) => err.to_string(),
            Ok(_) => panic!("expected a load error"),
        }
    }

    fn on_hit(effect: &str) -> ComponentDef {
        ComponentDef::OnHit(vec![OnHitDef {
            effect: effect.to_string(),
            chance_bp: 5000,
        }])
    }

    #[test]
    fn unknown_parent_is_reported() {
        assert_eq!(
            load_error(vec![("A", bp(Some("Nope"), vec![]))]),
            r#"blueprint "A" inherits from unknown "Nope""#
        );
    }

    /// The parent, which wrote the bad name, is reported, not the child
    /// that inherited it (which would come first in name order).
    #[test]
    fn unknown_equipment_is_reported_on_the_blueprint_that_lists_it() {
        assert_eq!(
            load_error(vec![
                ("Zed", with_equipment(bp(None, vec![]), Some(&["Nope"]))),
                ("Alpha", bp(Some("Zed"), vec![])),
            ]),
            r#"blueprint "Zed" is equipped with unknown "Nope""#
        );
    }

    #[test]
    fn unknown_on_hit_effect_is_reported() {
        assert_eq!(
            load_error(vec![
                ("Zed", bp(None, vec![on_hit("Nope")])),
                ("Alpha", bp(Some("Zed"), vec![])),
            ]),
            r#"blueprint "Zed" applies unknown effect "Nope" on hit"#
        );
    }

    fn timed() -> ComponentDef {
        ComponentDef::Duration(Duration(100))
    }

    #[test]
    fn an_on_hit_effect_without_a_duration_is_reported() {
        assert_eq!(
            load_error(vec![
                ("Knife", bp(None, vec![on_hit("Rock")])),
                ("Rock", bp(None, vec![])),
            ]),
            r#"blueprint "Knife" applies effect "Rock" on hit, which has no Duration"#
        );
    }

    #[test]
    fn an_on_hit_effect_may_inherit_its_duration() {
        let raw = BTreeMap::from([
            ("Knife".to_string(), bp(None, vec![on_hit("Cut")])),
            ("Timed".to_string(), bp(None, vec![timed()])),
            ("Cut".to_string(), bp(Some("Timed"), vec![])),
        ]);
        assert!(Blueprints::resolve(raw).is_ok());
    }

    #[test]
    fn equipment_cycles_are_reported() {
        assert_eq!(
            load_error(vec![
                ("A", with_equipment(bp(None, vec![]), Some(&["B"]))),
                ("B", with_equipment(bp(None, vec![]), Some(&["A"]))),
            ]),
            "equipment cycle: A -> B -> A"
        );
    }

    /// `Holster` only gets `Belt` by inheriting it.
    #[test]
    fn equipment_cycles_through_inherited_lists_are_reported() {
        assert_eq!(
            load_error(vec![
                ("Base", with_equipment(bp(None, vec![]), Some(&["Belt"]))),
                ("Belt", with_equipment(bp(None, vec![]), Some(&["Holster"]))),
                ("Holster", bp(Some("Base"), vec![])),
            ]),
            "equipment cycle: Belt -> Holster -> Belt"
        );
    }

    #[test]
    fn on_hit_parses_and_is_spawned_with_resolved_ids() {
        let def: Blueprint = ron_options()
            .from_str(r#"(components: [OnHit([(effect: "Cut", chance_bp: 5000)])])"#)
            .unwrap();
        assert_eq!(def.components, [on_hit("Cut")]);

        let blueprints = Blueprints::resolve(BTreeMap::from([
            ("Knife".to_string(), def),
            ("Cut".to_string(), bp(None, vec![timed()])),
        ]))
        .unwrap();
        let mut world = World::new();
        let knife = blueprints
            .spawn_id(&mut world, blueprints.id("Knife").unwrap())
            .unwrap();
        assert_eq!(
            *world.get::<&OnHit>(knife).unwrap(),
            OnHit(vec![OnHitEntry {
                effect: blueprints.id("Cut").unwrap(),
                chance_bp: 5000,
            }])
        );
    }

    #[test]
    fn equipment_is_spawned_and_attached() {
        let blueprints = Blueprints::load_dir(&data_dir()).unwrap();
        let mut world = World::new();
        let goblin = blueprints
            .spawn_id(&mut world, blueprints.id("Goblin").unwrap())
            .unwrap();
        let attachments = holding::attached(&world, goblin);
        assert_eq!(attachments.len(), 2);
        assert!(world.satisfies::<&Armor>(attachments[0]));
        assert!(world.satisfies::<&OnHit>(attachments[1]));
    }

    #[test]
    fn missing_directory_is_an_error() {
        assert!(Blueprints::load_dir(&data_dir().join("does-not-exist")).is_err());
    }
}
