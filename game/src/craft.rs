//! Crafting (docs/plans/crafting.md): materials with properties, what
//! creatures yield, and recipes over properties. Properties are listed in
//! `data/properties.ron`, recipes in `data/recipes.ron`; a material names its
//! own properties, and a product takes those of what went in.

use std::path::Path;

use hecs::Entity;
use rand::RngExt;
use serde::Deserialize;

use crate::blueprint::{BlueprintError, ron_options};
use crate::entity::{
    DamageType, Found, Material, Position, Projectile, Recipes, SpawnSeq, Weapon, Yields,
};
use crate::grammar::Subject;
use crate::holding;
use crate::spawn::Spawn;
use crate::state::GameState;

/// The most slots a recipe has, so a craft's choices fit in a `Command`.
pub const MAX_SLOTS: usize = 3;

/// One property a material can have.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyDef {
    pub name: String,
    /// What it means, in a few words, for looking at a material.
    pub note: String,
    /// What it gives a product made with it.
    #[serde(default)]
    pub gives: Vec<Give>,
}

/// What a property does to a product: small edits to the product's own
/// parts, so a crafted thing is an ordinary weapon or arrow to everything
/// that reads it. An edit to a part the product lacks does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Give {
    /// Added to a weapon's or a missile's damage, both ends.
    Damage(i32),
    /// A kind of damage its blows carry as well.
    Kind(DamageType),
}

/// A recipe: what it makes from materials filling its slots.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeDef {
    pub name: String,
    /// The product's blueprint, which the inputs' properties then edit.
    pub makes: String,
    /// Can be made anywhere, not only at a workbench.
    #[serde(default)]
    pub field: bool,
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    /// What the material is for ("edge"), for reading.
    pub role: String,
    /// The properties a material needs to fill it.
    pub needs: Vec<String>,
}

/// What crafting reads from the content.
#[derive(Debug, Clone, Default)]
pub struct Crafting {
    pub properties: Vec<PropertyDef>,
    pub recipes: Vec<RecipeDef>,
}

/// `data/properties.ron` and `data/recipes.ron`, either absent for none:
/// content without crafting needs neither.
pub(crate) fn read(data: &Path) -> Result<Crafting, BlueprintError> {
    let properties: Vec<PropertyDef> = read_list(&data.join("properties.ron"))?;
    let recipes: Vec<RecipeDef> = read_list(&data.join("recipes.ron"))?;
    let twice = |names: Vec<&String>, what: &str| {
        names
            .iter()
            .enumerate()
            .find(|(i, name)| names[..*i].contains(name))
            .map(|(_, name)| BlueprintError::BadCraft(format!("{what} {name:?} listed twice")))
    };
    if let Some(err) = twice(properties.iter().map(|p| &p.name).collect(), "property") {
        return Err(err);
    }
    if let Some(err) = twice(recipes.iter().map(|r| &r.name).collect(), "recipe") {
        return Err(err);
    }
    Ok(Crafting {
        properties,
        recipes,
    })
}

fn read_list<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, BlueprintError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text =
        std::fs::read_to_string(path).map_err(|err| BlueprintError::Io(path.to_path_buf(), err))?;
    ron_options()
        .from_str(&text)
        .map_err(|err| BlueprintError::Parse(path.to_path_buf(), Box::new(err)))
}

/// A craft that can go ahead: the recipe and the material filling each slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ready {
    pub recipe: u32,
    pub inputs: Vec<Entity>,
}

/// Whether `actor` can make recipe `recipe` now, and from what: the one
/// check, shared by the command, refused input and the recipe list. A slot
/// with a chosen material takes it; an empty one takes the first carried
/// material, by `SpawnSeq`, that fits and is not already used.
pub(crate) fn ready(
    state: &GameState,
    actor: Entity,
    recipe: u32,
    chosen: [Option<Entity>; MAX_SLOTS],
) -> Result<Ready, String> {
    let world = &state.world;
    let recipes = state.blueprints.recipes();
    let Some(def) = usize::try_from(recipe).ok().and_then(|i| recipes.get(i)) else {
        return Err("There is no such recipe.".into());
    };
    let known = world
        .get::<&Recipes>(actor)
        .is_ok_and(|known| known.0.contains(&def.name));
    if !known {
        return Err(format!(
            "You don't know how to make {}.",
            def.name.to_lowercase()
        ));
    }
    if !def.field && !at_workbench(state, actor) {
        return Err("You need a workbench for that.".into());
    }
    let mut carried: Vec<(SpawnSeq, Entity)> = holding::carried(world, actor)
        .into_iter()
        .filter_map(|thing| Some((*world.get::<&SpawnSeq>(thing).ok()?, thing)))
        .collect();
    carried.sort();
    let fits = |thing: Entity, slot: &Slot| {
        world
            .get::<&Material>(thing)
            .is_ok_and(|material| slot.needs.iter().all(|need| material.0.contains(need)))
    };
    let mut inputs: Vec<Entity> = Vec::with_capacity(def.slots.len());
    for (slot, choice) in def.slots.iter().zip(chosen) {
        let input = match choice {
            Some(thing) => {
                let noun = Subject::of(world, thing).noun().to_string();
                if !carried.iter().any(|&(_, c)| c == thing) {
                    return Err(format!("You do not carry {noun}."));
                }
                if inputs.contains(&thing) {
                    return Err(format!(
                        "{} already goes into another part.",
                        crate::grammar::capitalize(&noun)
                    ));
                }
                if !fits(thing, slot) {
                    return Err(format!(
                        "{} won't do for the {}: it must be {}.",
                        crate::grammar::capitalize(&noun),
                        slot.role,
                        slot.needs.join(" and ")
                    ));
                }
                thing
            }
            None => carried
                .iter()
                .map(|&(_, thing)| thing)
                .find(|&thing| !inputs.contains(&thing) && fits(thing, slot))
                .ok_or_else(|| {
                    format!(
                        "You have nothing {} for the {}.",
                        slot.needs.join(" and "),
                        slot.role
                    )
                })?,
        };
        inputs.push(input);
    }
    Ok(Ready { recipe, inputs })
}

/// On or beside a workbench.
fn at_workbench(state: &GameState, actor: Entity) -> bool {
    let Ok(at) = state.world.get::<&Position>(actor).map(|at| *at) else {
        return false;
    };
    (-1..=1).any(|dy| {
        (-1..=1).any(|dx| state.map.get(at.x + dx, at.y + dy) == Some(&crate::map::Tile::Workbench))
    })
}

/// `Command::Craft` applier: uses up the inputs and makes the product,
/// edited by every property of what went in.
pub(crate) fn craft(
    state: &mut GameState,
    actor: Entity,
    recipe: u32,
    chosen: [Option<Entity>; MAX_SLOTS],
) {
    let ready = match ready(state, actor, recipe, chosen) {
        Ok(ready) => ready,
        Err(why) => {
            state.log.push(why);
            return;
        }
    };
    let Some(def) = usize::try_from(ready.recipe)
        .ok()
        .and_then(|i| state.blueprints.recipes().get(i))
        .cloned()
    else {
        return;
    };
    let mut properties: Vec<String> = ready
        .inputs
        .iter()
        .filter_map(|&input| {
            state
                .world
                .get::<&Material>(input)
                .ok()
                .map(|m| m.0.clone())
        })
        .flatten()
        .collect();
    properties.sort();
    properties.dedup();
    for &input in &ready.inputs {
        holding::unstow(&mut state.world, actor, input);
        crate::command::despawn_with_holdings(state, input);
    }
    let Ok(product) = state.spawn(Spawn::blueprint(&def.makes).carried_by(actor)) else {
        return;
    };
    let gives: Vec<Give> = properties
        .iter()
        .filter_map(|name| {
            state
                .blueprints
                .properties()
                .iter()
                .find(|p| &p.name == name)
        })
        .flat_map(|property| property.gives.clone())
        .collect();
    for give in gives {
        apply(state, product, give);
    }
    let made = Subject::of(&state.world, product);
    let line = Subject::of(&state.world, actor).sentence("make", made.noun());
    state.log.push(line);
}

fn apply(state: &mut GameState, product: Entity, give: Give) {
    let world = &mut state.world;
    match give {
        Give::Damage(more) => {
            if let Ok(mut weapon) = world.get::<&mut Weapon>(product) {
                weapon.min += more;
                weapon.max += more;
            }
            if let Ok(mut missile) = world.get::<&mut Projectile>(product) {
                missile.min += more;
                missile.max += more;
            }
        }
        Give::Kind(kind) => {
            let add = |types: crate::entity::DamageTypes| {
                types
                    .iter()
                    .chain([kind])
                    .collect::<crate::entity::DamageTypes>()
            };
            if let Ok(mut weapon) = world.get::<&mut Weapon>(product) {
                weapon.types = add(weapon.types);
            }
            if let Ok(mut missile) = world.get::<&mut Projectile>(product) {
                missile.types = add(missile.types);
            }
        }
    }
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
    use crate::blueprint::ComponentDef;
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
    fn a_name_crafting_uses_that_names_nothing_fails_to_load() {
        let data = crate::testkit::data();
        let blueprints = crate::blueprint::Blueprints::load(&data).unwrap();
        let shipped = Crafting {
            properties: blueprints.properties().to_vec(),
            recipes: blueprints.recipes().to_vec(),
        };
        assert!(blueprints.clone().with_crafting(shipped.clone()).is_ok());
        let no_properties = Crafting {
            properties: Vec::new(),
            ..shipped.clone()
        };
        assert!(
            matches!(
                blueprints.clone().with_crafting(no_properties),
                Err(BlueprintError::BadCraft(_))
            ),
            "materials and recipes name properties no longer listed"
        );
        let no_recipes = Crafting {
            recipes: Vec::new(),
            ..shipped
        };
        let knows_one = blueprints
            .origins()
            .iter()
            .filter_map(|origin| blueprints.get(&origin.blueprint))
            .any(|b| {
                b.components
                    .iter()
                    .any(|def| matches!(def, ComponentDef::Recipes(r) if !r.0.is_empty()))
            });
        if knows_one {
            assert!(
                blueprints.clone().with_crafting(no_recipes).is_err(),
                "an origin knows a recipe no longer listed"
            );
        }
    }

    /// The bare room, the player knowing `recipes` (index order), carrying
    /// one each of `materials`, by property lists.
    fn crafter(recipes: Vec<RecipeDef>, materials: &[&[&str]]) -> (GameState, Vec<Entity>) {
        let mut state = bare_state();
        let properties = ["hard", "flexible", "cold"]
            .map(|name| PropertyDef {
                name: name.into(),
                note: String::new(),
                gives: match name {
                    "hard" => vec![Give::Damage(1)],
                    "cold" => vec![Give::Kind(DamageType::Cold)],
                    _ => Vec::new(),
                },
            })
            .to_vec();
        let known = Recipes(recipes.iter().map(|r| r.name.clone()).collect());
        state.blueprints = state.blueprints.clone().with_crafting_unchecked(Crafting {
            properties,
            recipes,
        });
        let player = state.player;
        let _ = state.world.insert_one(player, known);
        let held = materials
            .iter()
            .map(|props| {
                thing("scrap")
                    .item()
                    .with(Material(props.iter().map(|p| p.to_string()).collect()))
                    .carried_by(player)
                    .make(&mut state)
            })
            .collect();
        (state, held)
    }

    fn shiv() -> RecipeDef {
        RecipeDef {
            name: "Shiv".into(),
            makes: "Shiv".into(),
            field: true,
            slots: vec![
                Slot {
                    role: "edge".into(),
                    needs: vec!["hard".into()],
                },
                Slot {
                    role: "grip".into(),
                    needs: vec!["flexible".into()],
                },
            ],
        }
    }

    fn weapon_carried(state: &GameState) -> Option<Weapon> {
        holding::carried(&state.world, state.player)
            .into_iter()
            .find_map(|thing| state.world.get::<&Weapon>(thing).ok().map(|w| *w))
    }

    fn craft_now(state: &mut GameState, chosen: [Option<Entity>; MAX_SLOTS]) -> bool {
        let player = state.player;
        crate::time::submit(
            state,
            crate::command::Command::Craft {
                actor: player,
                recipe: 0,
                inputs: chosen,
            },
        )
    }

    #[test]
    fn a_craft_uses_up_its_inputs_and_the_product_takes_their_properties() {
        let (mut state, held) =
            crafter(vec![shiv()], &[&["flexible"], &["hard", "cold"], &["hard"]]);
        assert!(craft_now(&mut state, [None; MAX_SLOTS]));
        let carried = holding::carried(&state.world, state.player);
        assert!(
            !carried.contains(&held[0]) && !carried.contains(&held[1]),
            "the first that fit"
        );
        assert!(carried.contains(&held[2]), "the rest is kept");
        let shiv = weapon_carried(&state).unwrap();
        assert_eq!((shiv.min, shiv.max), (2, 3), "hard: one more damage");
        assert!(shiv.types.contains(DamageType::Cold), "a cold edge");
        assert_eq!(crate::testkit::last_log(&state), "You make the shiv.");
    }

    #[test]
    fn a_chosen_material_fills_its_slot_and_the_rest_fill_themselves() {
        let (mut state, held) =
            crafter(vec![shiv()], &[&["hard", "cold"], &["hard"], &["flexible"]]);
        assert!(craft_now(&mut state, [Some(held[1]), None, None]));
        let carried = holding::carried(&state.world, state.player);
        assert!(carried.contains(&held[0]), "the cold one was spared");
        assert!(
            !weapon_carried(&state)
                .unwrap()
                .types
                .contains(DamageType::Cold)
        );
    }

    #[test]
    fn only_a_field_recipe_is_made_away_from_a_workbench() {
        let bench = RecipeDef {
            field: false,
            ..shiv()
        };
        let (mut state, _) = crafter(vec![bench], &[&["hard"], &["flexible"]]);
        assert!(!craft_now(&mut state, [None; MAX_SLOTS]));
        assert_eq!(
            crate::testkit::last_log(&state),
            "You need a workbench for that."
        );
        // The player stands at (1, 1); a bench beside them will do.
        state.map.set(2, 2, crate::map::Tile::Workbench);
        assert!(craft_now(&mut state, [None; MAX_SLOTS]));
    }

    #[test]
    fn a_craft_that_cannot_go_ahead_is_refused_and_takes_no_time() {
        // Carried materials (by properties), which of them each slot gets, the refusal.
        type Case<'a> = (&'a [&'a [&'a str]], [Option<usize>; MAX_SLOTS], &'a str);
        let cases: [Case; 3] = [
            (
                &[&["hard"]],
                [None; MAX_SLOTS],
                "You have nothing flexible for the grip.",
            ),
            (
                &[&["flexible"], &["hard"]],
                [Some(0), None, None],
                "The scrap won't do for the edge: it must be hard.",
            ),
            (
                &[&["hard"], &["flexible"]],
                [Some(0), Some(0), None],
                "The scrap already goes into another part.",
            ),
        ];
        for (materials, chosen, why) in cases {
            let (mut state, held) = crafter(vec![shiv()], materials);
            let before = (state.now, holding::carried(&state.world, state.player));
            let chosen = chosen.map(|index| index.map(|i| held[i]));
            assert!(!craft_now(&mut state, chosen), "{why}");
            assert_eq!(crate::testkit::last_log(&state), why);
            assert_eq!(
                (state.now, holding::carried(&state.world, state.player)),
                before
            );
        }
        let (mut state, _) = crafter(vec![shiv()], &[&["hard"], &["flexible"]]);
        let player = state.player;
        let _ = state.world.insert_one(player, Recipes(Vec::new()));
        assert!(!craft_now(&mut state, [None; MAX_SLOTS]));
        assert_eq!(
            crate::testkit::last_log(&state),
            "You don't know how to make shiv."
        );
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
}
