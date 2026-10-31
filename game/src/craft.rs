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
    DamageType, Found, Material, Position, Projectile, Quality, Recipes, SpawnSeq, Weapon, Yields,
};
use crate::grammar::Subject;
use crate::holding;
use crate::spawn::Spawn;
use crate::state::GameState;
use crate::view::ItemRef;

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
    /// The skills it uses, by name: trained by it, and their level against
    /// `difficulty` decides how well it turns out.
    #[serde(default)]
    pub skills: Vec<String>,
    /// The level at which it comes out plain most of the time.
    #[serde(default)]
    pub difficulty: u32,
}

/// In percent, from the craft's level less the recipe's difficulty: crude
/// below it more often, fine above it sometimes.
fn quality(over: i64, roll: i64) -> Quality {
    let crude = (30 - 10 * over).clamp(0, 90);
    let fine = (5 * over).clamp(0, 50);
    if roll < crude {
        Quality::Crude
    } else if roll >= 100 - fine {
        Quality::Fine
    } else {
        Quality::Plain
    }
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

/// A recipe as a frontend shows it: what it makes, and for each slot what
/// fits and what would be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeView {
    /// What `PlayerAction::Craft` names it by.
    pub recipe: u32,
    pub name: String,
    /// The product's noun ("arrow").
    pub makes: String,
    /// Made anywhere, not only at a workbench.
    pub field: bool,
    pub slots: Vec<SlotView>,
    /// Why it can't be made now, as `craft::ready` says; `None` if it can.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotView {
    pub role: String,
    pub needs: Vec<String>,
    /// Every carried material that fits, oldest first, with its noun.
    pub fits: Vec<(ItemRef, String)>,
    /// What would fill it, if the craft can go ahead.
    pub uses: Option<String>,
}

/// Every known recipe, those that can be made now first (then in list
/// order), each with nothing chosen by hand.
pub fn recipes(state: &GameState) -> Vec<RecipeView> {
    let known = state
        .world
        .get::<&Recipes>(state.player)
        .map(|known| known.0.clone())
        .unwrap_or_default();
    let mut views: Vec<RecipeView> = (0..)
        .zip(state.blueprints.recipes())
        .filter(|(_, def)| known.contains(&def.name))
        .filter_map(|(index, _)| recipe(state, index, [None; MAX_SLOTS]))
        .collect();
    views.sort_by_key(|view| view.blocked.is_some());
    views
}

/// Recipe `index` with `chosen` materials for its slots, as it would go.
pub fn recipe(
    state: &GameState,
    index: u32,
    chosen: [Option<ItemRef>; MAX_SLOTS],
) -> Option<RecipeView> {
    let def = state
        .blueprints
        .recipes()
        .get(usize::try_from(index).ok()?)?;
    let world = &state.world;
    let player = state.player;
    let carried = holding::carried(world, player);
    let mut found = [None; MAX_SLOTS];
    for (slot, choice) in found.iter_mut().zip(chosen) {
        *slot = choice.and_then(|choice| choice.find(world, &carried));
    }
    let ready = ready(state, player, index, found);
    let noun = |thing: Entity| Subject::of(world, thing).noun().to_string();
    let mut materials: Vec<(SpawnSeq, Entity)> = carried
        .iter()
        .filter_map(|&thing| Some((*world.get::<&SpawnSeq>(thing).ok()?, thing)))
        .collect();
    materials.sort();
    let slots = def
        .slots
        .iter()
        .enumerate()
        .map(|(i, slot)| SlotView {
            role: slot.role.clone(),
            needs: slot.needs.clone(),
            fits: materials
                .iter()
                .filter(|&&(_, thing)| {
                    world
                        .get::<&Material>(thing)
                        .is_ok_and(|m| slot.needs.iter().all(|need| m.0.contains(need)))
                })
                .map(|&(seq, thing)| (ItemRef(seq), noun(thing)))
                .collect(),
            uses: ready
                .as_ref()
                .ok()
                .and_then(|r| r.inputs.get(i))
                .map(|&t| noun(t)),
        })
        .collect();
    let makes = state
        .blueprints
        .get(&def.makes)
        .and_then(|b| b.noun())
        .unwrap_or(&def.makes)
        .to_string();
    Some(RecipeView {
        recipe: index,
        name: def.name.clone(),
        makes,
        field: def.field,
        slots,
        blocked: ready.err(),
    })
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
    let book = state.blueprints.skills().clone();
    let skills: Vec<_> = def.skills.iter().filter_map(|name| book.id(name)).collect();
    let level = crate::skill::effective(&state.world, &book, actor, &skills);
    let over = i64::from(level) - i64::from(def.difficulty);
    let made = quality(over, state.rngs.craft.random_range(0..100));
    // Each material trains as its own kind of thing, so a new one teaches
    // and the hundredth of the same does not (ADR 0017).
    let share = (crate::skill::BASE_XP / u32::try_from(ready.inputs.len()).unwrap_or(1)).max(1);
    for &input in &ready.inputs {
        crate::skill::grow(state, actor, &skills, input, share);
    }
    for &input in &ready.inputs {
        holding::unstow(&mut state.world, actor, input);
        crate::command::despawn_with_holdings(state, input);
    }
    let Ok(product) = state.spawn(Spawn::blueprint(&def.makes).carried_by(actor)) else {
        return;
    };
    set_quality(state, product, made);
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
    let thing = Subject::of(&state.world, product);
    let line = Subject::of(&state.world, actor).sentence("make", thing.noun());
    state.log.push(line);
    // Working something rare can inspire; common materials never do.
    if properties.iter().any(|property| property == "rare") {
        crate::discovery::inspire(state, actor);
    }
}

/// Marks `product` with how it turned out: one off its damage or armor, and
/// its name says so unless it is plain.
fn set_quality(state: &mut GameState, product: Entity, made: Quality) {
    let (shift, word) = match made {
        Quality::Crude => (-1, Some("crude")),
        Quality::Plain => (0, None),
        Quality::Fine => (1, Some("fine")),
    };
    let world = &mut state.world;
    if let Ok(mut weapon) = world.get::<&mut Weapon>(product) {
        weapon.max = (weapon.max + shift).max(1);
        weapon.min = (weapon.min + shift).clamp(0, weapon.max);
    }
    if let Ok(mut missile) = world.get::<&mut Projectile>(product) {
        missile.max = (missile.max + shift).max(1);
        missile.min = (missile.min + shift).clamp(0, missile.max);
    }
    if let Ok(mut armor) = world.get::<&mut crate::entity::Armor>(product) {
        let armor = &mut *armor;
        for value in [&mut armor.slashing, &mut armor.blunt, &mut armor.piercing] {
            if *value > 0 {
                *value = (*value + shift).max(0);
            }
        }
    }
    if let (Some(word), Ok(mut name)) = (word, world.get::<&mut crate::entity::Name>(product)) {
        name.0 = format!("{word} {}", name.0);
    }
    let _ = world.insert_one(product, made);
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
    let player = state.player;
    let scavenging = state.blueprints.skills().core().map(|core| core.scavenging);
    let level = scavenging.map_or(0, |skill| {
        crate::skill::level(crate::skill::xp(&state.world, player, skill))
    });
    for entry in yields {
        let chance = entry.chance_bp + level * SCAVENGING_BP_PER_LEVEL;
        if state.rngs.craft.random_range(0..CERTAIN_BP) >= chance {
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
            // Picking over what a fight left trains Scavenging, by the kind
            // of creature (ADR 0017).
            if let Some(skill) = scavenging {
                crate::skill::train(state, player, &[skill], dead, crate::skill::BASE_XP / 2);
            }
        }
    }
}

/// What a level of Scavenging adds to every yield's chance.
const SCAVENGING_BP_PER_LEVEL: u32 = 250;

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
    fn scavenging_grows_on_what_kills_leave_and_makes_them_leave_more() {
        let left = |level: u32| {
            let mut total = 0;
            for seed in 0..100 {
                let mut state = bare_state();
                crate::testkit::see_everything(&mut state);
                state.rngs.craft = rand_pcg::Pcg64::new(seed, crate::rng::stream::CRAFT);
                let core = state.blueprints.skills().core().unwrap();
                let mut skills = crate::entity::Skills::default();
                skills.set(core.scavenging, crate::skill::xp_for(level));
                let player = state.player;
                let _ = state.world.insert(player, (crate::entity::Learner, skills));
                let goblin = yielder(&mut state, 2500);
                state.run(Command::Die { entity: goblin });
                total += bones_at(&state, (2, 1));
                if total > 0 && level == 0 && bones_at(&state, (2, 1)) > 0 {
                    let xp = crate::skill::xp(&state.world, player, core.scavenging);
                    assert!(xp > 0, "a yield seen trains Scavenging");
                }
            }
            total
        };
        assert!(left(10) > left(0));
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
            skills: Vec::new(),
            difficulty: 0,
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
        let made = holding::carried(&state.world, state.player)
            .into_iter()
            .find(|&thing| state.world.satisfies::<&Weapon>(thing))
            .unwrap();
        let shift = match *state.world.get::<&Quality>(made).unwrap() {
            Quality::Crude => -1,
            Quality::Plain => 0,
            Quality::Fine => 1,
        };
        let shiv = weapon_carried(&state).unwrap();
        // The shiv is 1-2; hard adds one, then the quality shifts it.
        assert_eq!(
            (shiv.min, shiv.max),
            (2 + shift, 3 + shift),
            "hard: one more damage"
        );
        assert!(shiv.types.contains(DamageType::Cold), "a cold edge");
        assert!(crate::testkit::last_log(&state).starts_with("You make the "));
    }

    #[test]
    fn a_skilled_hand_makes_crude_things_less_often_and_fine_ones_more() {
        let count = |level: u32| {
            let mut crude_fine = (0, 0);
            for seed in 0..200 {
                let mut recipe = shiv();
                recipe.skills = vec!["Dagger".into()];
                recipe.difficulty = 3;
                let (mut state, _) = crafter(vec![recipe], &[&["hard"], &["flexible"]]);
                state.rngs.craft = rand_pcg::Pcg64::new(seed, crate::rng::stream::CRAFT);
                let dagger = state.blueprints.skills().id("Dagger").unwrap();
                let mut skills = crate::entity::Skills::default();
                skills.set(dagger, crate::skill::xp_for(level));
                let player = state.player;
                let _ = state.world.insert_one(player, skills);
                assert!(craft_now(&mut state, [None; MAX_SLOTS]));
                let made = holding::carried(&state.world, player)
                    .into_iter()
                    .find_map(|thing| state.world.get::<&Quality>(thing).ok().map(|q| *q))
                    .unwrap();
                match made {
                    Quality::Crude => crude_fine.0 += 1,
                    Quality::Fine => crude_fine.1 += 1,
                    Quality::Plain => {}
                }
            }
            crude_fine
        };
        let (novice, master) = (count(0), count(9));
        assert!(novice.0 > master.0, "crude: {novice:?} against {master:?}");
        assert!(novice.1 < master.1, "fine: {novice:?} against {master:?}");
    }

    #[test]
    fn a_new_material_teaches_more_than_a_known_one() {
        let mut recipe = shiv();
        recipe.skills = vec!["Dagger".into()];
        let materials: &[&[&str]] = &[&["hard"], &["flexible"], &["hard"], &["flexible"]];
        let (mut state, held) = crafter(vec![recipe], materials);
        let player = state.player;
        let _ = state.world.insert_one(player, crate::entity::Learner);
        let dagger = state.blueprints.skills().id("Dagger").unwrap();
        let xp = |state: &GameState| crate::skill::xp(&state.world, player, dagger);
        // The second pair is named differently: new kinds of material.
        for &thing in &held[2..] {
            let _ = state.world.insert_one(thing, Name("rag".into()));
        }
        assert!(craft_now(&mut state, [Some(held[0]), Some(held[1]), None]));
        let first = xp(&state);
        assert!(craft_now(&mut state, [None; MAX_SLOTS]));
        let fresh = xp(&state) - first;
        let (mut again, held) = crafter(
            vec![RecipeDef {
                skills: vec!["Dagger".into()],
                ..shiv()
            }],
            materials,
        );
        let _ = again.world.insert_one(player, crate::entity::Learner);
        assert!(craft_now(&mut again, [Some(held[0]), Some(held[1]), None]));
        let first_again = xp(&again);
        assert!(craft_now(&mut again, [None; MAX_SLOTS]));
        let repeated = xp(&again) - first_again;
        assert!(
            fresh > repeated,
            "new material {fresh}, the same again {repeated}"
        );
    }

    #[test]
    fn working_a_rare_material_can_inspire_and_a_common_one_never_does() {
        // A discovery from the real data whose skill requirements are met.
        let data = crate::blueprint::Blueprints::load(&crate::testkit::data()).unwrap();
        let Some(discovery) = data.discoveries().iter().find(|d| d.phase.is_none()) else {
            return;
        };
        let (name, requires) = (discovery.name.clone(), discovery.requires.clone());
        let ready_to_be_inspired = |rare: bool| {
            let mut state = crate::state::tests::bare_state_with(data.clone());
            let player = state.player;
            let mut skills = crate::entity::Skills::default();
            for &(skill, level) in &requires {
                skills.set(skill, crate::skill::xp_for(level));
            }
            let _ = state.world.insert(player, (crate::entity::Learner, skills));
            let recipe = RecipeDef {
                makes: "Knife".into(),
                ..shiv()
            };
            let properties = ["hard", "flexible", "rare"]
                .map(|name| PropertyDef {
                    name: name.into(),
                    note: String::new(),
                    gives: Vec::new(),
                })
                .to_vec();
            state.blueprints = state.blueprints.clone().with_crafting_unchecked(Crafting {
                properties,
                recipes: vec![recipe],
            });
            let _ = state.world.insert_one(player, Recipes(vec!["Shiv".into()]));
            let edge: &[&str] = if rare { &["hard", "rare"] } else { &["hard"] };
            for props in [edge, &["flexible"][..]] {
                thing("scrap")
                    .item()
                    .with(Material(props.iter().map(|p| p.to_string()).collect()))
                    .carried_by(player)
                    .make(&mut state);
            }
            assert!(craft_now(&mut state, [None; MAX_SLOTS]));
            crate::discovery::made(&state, player).contains(&name)
        };
        assert!(
            !ready_to_be_inspired(false),
            "common materials never inspire"
        );
        assert!(ready_to_be_inspired(true), "a rare one can");
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
