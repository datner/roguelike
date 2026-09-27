//! Data-driven entity definitions, loaded from `data/entities/*.ron`. A
//! blueprint's name is its file stem. Inheritance is resolved once at load
//! time, so spawning never walks the parent chain.
//!
//! Inheritance follows one rule: the child's field wins. Each component
//! variant counts as its own field (a child's `Health` replaces the parent's,
//! other parent components are kept), and `equipment` is a single field
//! (absent inherits the parent's list, present replaces it, `[]` clears it).

use std::collections::BTreeMap;
use std::fs;
use std::mem::discriminant;
use std::path::{Path, PathBuf};

use hecs::{Entity, EntityBuilder, World};
use ron::extensions::Extensions;
use serde::{Deserialize, Serialize};

use crate::entity::{
    Appearance, Armor, Attachments, Bleeding, BlocksMovement, Duration, Health, Hostile, Melee,
    Name, Player, Position, Speed,
};

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
    Bleeding(Bleeding),
    Duration(Duration),
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
            ComponentDef::Bleeding(c) => builder.add(c),
            ComponentDef::Duration(c) => builder.add(c),
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

/// A set of blueprints with inheritance already flattened: every stored
/// blueprint has `inherits: None`.
#[derive(Debug, Clone, Default)]
pub struct Blueprints {
    resolved: BTreeMap<String, Blueprint>,
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
    /// Names along the cycle, starting and ending with the same blueprint.
    InheritanceCycle(Vec<String>),
    EquipmentCycle(Vec<String>),
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
            Self::InheritanceCycle(path) => write!(f, "inheritance cycle: {}", path.join(" -> ")),
            Self::EquipmentCycle(path) => write!(f, "equipment cycle: {}", path.join(" -> ")),
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

    /// Flattens inheritance and checks that every referenced name exists.
    pub fn resolve(raw: BTreeMap<String, Blueprint>) -> Result<Self, BlueprintError> {
        let mut resolved = BTreeMap::new();
        for name in raw.keys() {
            resolve_one(name, &raw, &mut resolved, &mut Vec::new())?;
        }
        for (owner, blueprint) in &resolved {
            if let Some(item) = blueprint
                .equipment()
                .iter()
                .find(|item| !resolved.contains_key(*item))
            {
                return Err(BlueprintError::UnknownEquipment {
                    owner: owner.clone(),
                    item: item.clone(),
                });
            }
        }
        Ok(Self { resolved })
    }

    pub fn get(&self, name: &str) -> Option<&Blueprint> {
        self.resolved.get(name)
    }

    /// Spawns `name`, and each piece of its equipment as a separate entity
    /// listed in the owner's `Attachments`.
    pub fn spawn(&self, world: &mut World, name: &str) -> Result<Entity, BlueprintError> {
        self.spawn_inner(world, name, &mut Vec::new())
    }

    pub fn spawn_at(
        &self,
        world: &mut World,
        name: &str,
        at: Position,
    ) -> Result<Entity, BlueprintError> {
        let entity = self.spawn(world, name)?;
        world
            .insert_one(entity, at)
            .expect("entity was just spawned");
        Ok(entity)
    }

    fn spawn_inner(
        &self,
        world: &mut World,
        name: &str,
        stack: &mut Vec<String>,
    ) -> Result<Entity, BlueprintError> {
        if stack.iter().any(|n| n == name) {
            let mut cycle = stack.clone();
            cycle.push(name.to_string());
            return Err(BlueprintError::EquipmentCycle(cycle));
        }
        let blueprint = self
            .get(name)
            .ok_or_else(|| BlueprintError::UnknownBlueprint(name.to_string()))?;

        stack.push(name.to_string());
        let mut attachments = Vec::with_capacity(blueprint.equipment().len());
        for item in blueprint.equipment() {
            attachments.push(self.spawn_inner(world, item, stack)?);
        }
        stack.pop();

        let mut builder = EntityBuilder::new();
        for component in &blueprint.components {
            component.add_to(&mut builder);
        }
        if !attachments.is_empty() {
            builder.add(Attachments(attachments));
        }
        Ok(world.spawn(builder.build()))
    }
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
    fn resolve_child_equipment(child_equipment: Option<&[&str]>) -> Vec<String> {
        let raw = BTreeMap::from([
            ("Sword".to_string(), bp(None, vec![])),
            ("Shield".to_string(), bp(None, vec![])),
            (
                "Base".to_string(),
                with_equipment(bp(None, vec![]), Some(&["Sword"])),
            ),
            (
                "Child".to_string(),
                with_equipment(bp(Some("Base"), vec![]), child_equipment),
            ),
        ]);
        Blueprints::resolve(raw)
            .unwrap()
            .get("Child")
            .unwrap()
            .equipment()
            .to_vec()
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
            "Rubble",
        ] {
            assert!(blueprints.get(name).is_some(), "missing {name}");
        }
        assert_eq!(
            blueprints.get("Goblin").unwrap().equipment(),
            ["LeatherArmor"]
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
        assert_eq!(child.inherits, None);
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

    #[test]
    fn unknown_parent_is_reported() {
        let raw = BTreeMap::from([("A".to_string(), bp(Some("Nope"), vec![]))]);
        assert!(matches!(
            Blueprints::resolve(raw),
            Err(BlueprintError::UnknownParent { .. })
        ));
    }

    #[test]
    fn equipment_is_spawned_and_attached() {
        let blueprints = Blueprints::load_dir(&data_dir()).unwrap();
        let mut world = World::new();
        let goblin = blueprints.spawn(&mut world, "Goblin").unwrap();
        let attachments = world.get::<&Attachments>(goblin).unwrap().0.clone();
        assert_eq!(attachments.len(), 1);
        assert!(world.satisfies::<&Armor>(attachments[0]));
    }

    #[test]
    fn missing_directory_is_an_error() {
        assert!(Blueprints::load_dir(&data_dir().join("does-not-exist")).is_err());
    }
}
