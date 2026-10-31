//! Crafting (docs/plans/crafting.md): materials with properties, what
//! creatures yield, and (later slices) recipes over properties. Properties
//! are listed in `data/properties.ron`; a material names its own.

use std::path::Path;

use hecs::Entity;
use rand::RngExt;
use serde::Deserialize;

use crate::blueprint::{BlueprintError, ron_options};
use crate::entity::{Found, Position, Yields};
use crate::grammar::Subject;
use crate::spawn::Spawn;
use crate::state::GameState;

/// One property a material can have.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyDef {
    pub name: String,
    /// What it means, in a few words, for looking at a material.
    pub note: String,
}

/// `data/properties.ron`, or none: content without crafting needs no file.
pub(crate) fn read(data: &Path) -> Result<Vec<PropertyDef>, BlueprintError> {
    let path = data.join("properties.ron");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text =
        std::fs::read_to_string(&path).map_err(|err| BlueprintError::Io(path.clone(), err))?;
    let properties: Vec<PropertyDef> = ron_options()
        .from_str(&text)
        .map_err(|err| BlueprintError::Parse(path.clone(), Box::new(err)))?;
    for (i, property) in properties.iter().enumerate() {
        if properties[..i]
            .iter()
            .any(|other| other.name == property.name)
        {
            return Err(BlueprintError::BadCraft(format!(
                "property {:?} listed twice",
                property.name
            )));
        }
    }
    Ok(properties)
}

/// Basis points: 10000 is certain.
const CERTAIN_BP: u32 = 10_000;

/// `dead` leaves what its `Yields` lists on its tile, each at its chance.
/// Called by `die` before the body is gone.
pub(crate) fn leave_yields(state: &mut GameState, dead: Entity) {
    let Ok(at) = state.world.get::<&Position>(dead).map(|position| *position) else {
        return;
    };
    let Some(yields) = state.world.get::<&Yields>(dead).ok().map(|y| y.0.clone()) else {
        return;
    };
    let seen = state.visible.get(at.x, at.y) == Some(&true);
    for entry in yields {
        if state.rngs.craft.random_range(0..CERTAIN_BP) >= entry.chance_bp {
            continue;
        }
        let Ok(thing) = state.spawn(Spawn::blueprint(&entry.blueprint).at(at.x, at.y)) else {
            continue;
        };
        if seen {
            // Seen falling, so it is found, like a dropped item.
            let _ = state.world.insert_one(thing, Found);
            let left = Subject::of(&state.world, thing);
            let line = Subject::of(&state.world, dead).sentence("leave", left.noun());
            state.log.push(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::entity::{Item, Name, Yield};
    use crate::state::tests::bare_state;
    use crate::testkit::thing;

    fn bones_at(state: &GameState, at: (i32, i32)) -> usize {
        state
            .world
            .query::<(&Name, &Position)>()
            .iter()
            .filter(|(name, pos)| name.0 == "bone" && (pos.x, pos.y) == at)
            .count()
    }

    fn yielder(state: &mut GameState, chance_bp: u32) -> Entity {
        thing("goblin")
            .at(2, 1)
            .health(1)
            .hostile()
            .with(Yields(vec![Yield {
                blueprint: "Bone".into(),
                chance_bp,
            }]))
            .make(state)
    }

    #[test]
    fn a_death_leaves_what_it_yields_where_it_fell() {
        let mut state = bare_state();
        let goblin = yielder(&mut state, CERTAIN_BP);
        state.run(Command::Die { entity: goblin });
        assert_eq!(bones_at(&state, (2, 1)), 1);
        let never = yielder(&mut state, 0);
        state.run(Command::Die { entity: never });
        assert_eq!(bones_at(&state, (2, 1)), 1, "a zero chance never yields");
        let bone = state
            .world
            .query::<(Entity, &Name)>()
            .iter()
            .find(|(_, name)| name.0 == "bone")
            .map(|(entity, _)| entity)
            .unwrap();
        assert!(state.world.satisfies::<&Item>(bone));
    }

    #[test]
    fn looking_at_a_material_names_its_properties() {
        let mut state = bare_state();
        crate::testkit::see_everything(&mut state);
        state.spawn(Spawn::blueprint("Bone").at(3, 1)).unwrap();
        let seen = crate::lore::inspect(&state, (3, 1)).unwrap();
        assert_eq!(
            (seen.name.as_str(), seen.lines.as_slice()),
            ("bone", &["hard".to_string()][..])
        );
    }

    #[test]
    fn a_material_or_a_yield_that_names_nothing_fails_to_load() {
        let data = crate::testkit::data();
        let blueprints = crate::blueprint::Blueprints::load(&data).unwrap();
        let none = blueprints.clone().with_properties(Vec::new());
        assert!(
            matches!(none, Err(BlueprintError::BadCraft(_))),
            "a material names a property that is no longer listed"
        );
        let ok = blueprints
            .clone()
            .with_properties(blueprints.properties().to_vec());
        assert!(ok.is_ok());
    }

    #[test]
    fn no_material_is_a_random_find() {
        let blueprints = crate::blueprint::Blueprints::load(&crate::testkit::data()).unwrap();
        let material = |id| {
            blueprints
                .by_id(id)
                .unwrap()
                .components
                .iter()
                .any(|def| matches!(def, crate::blueprint::ComponentDef::Material(_)))
        };
        assert!(!blueprints.items().into_iter().any(material));
    }

    #[test]
    fn the_shipped_materials_name_known_properties() {
        let blueprints = crate::blueprint::Blueprints::load(&crate::testkit::data()).unwrap();
        assert!(!blueprints.properties().is_empty());
    }
}
