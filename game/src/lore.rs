//! Lore (ADR 0017): what the player knows about the creatures they meet.
//! Beast lore grows by novelty, the first sight of each kind of creature,
//! and decides how much the look cursor reveals (`inspect`); Arcana grows
//! by the first cast of each spell and reads the spells a creature knows.

use hecs::Entity;

use crate::entity::{
    Attuned, DamageType, DamageTypes, Dead, Health, Item, Learner, Name, Position, Sighted, Spell,
    SpellDamage,
};
use crate::grammar::Subject;
use crate::skill;
use crate::state::GameState;
use crate::view::total_armor;

/// XP the first sight of a kind of creature teaches.
const FIRST_SIGHT_XP: u32 = 30;
/// Beast lore needed to read a creature's health, and its armor.
const HEALTH_FROM: u32 = 1;
const ARMOR_FROM: u32 = 3;

/// After the player's view is refreshed: every kind of creature in view
/// for the first time teaches Beast lore. Novelty, not stakes: it never
/// inspires a discovery.
pub(crate) fn sight(state: &mut GameState) {
    let player = state.player;
    let Some(core) = state.blueprints.skills().core() else {
        return;
    };
    if !state.world.satisfies::<&Learner>(player) {
        return;
    }
    let mut sighted = state
        .world
        .get::<&Sighted>(player)
        .map(|s| (*s).clone())
        .unwrap_or_default();
    let mut new: Vec<String> = state
        .world
        .query::<(hecs::Entity, &Position, &Name, &Health)>()
        .without::<&Dead>()
        .iter()
        .filter(|&(entity, pos, ..)| {
            entity != player && state.visible.get(pos.x, pos.y) == Some(&true)
        })
        .map(|(_, _, name, _)| name.0.clone())
        .filter(|name| !sighted.0.contains(name))
        .collect();
    new.sort();
    new.dedup();
    if new.is_empty() {
        return;
    }
    let xp = FIRST_SIGHT_XP * u32::try_from(new.len()).unwrap_or(u32::MAX);
    sighted.0.extend(new);
    sighted.0.sort();
    let _ = state.world.insert_one(player, sighted);
    skill::gain(state, player, &[(core.beast_lore, xp)]);
}

/// XP the first cast of each spell teaches Arcana.
const FIRST_CAST_XP: u32 = 30;
/// Arcana needed to tell which spells a creature knows, and their damage.
const SPELLS_FROM: u32 = 1;
const SPELL_DAMAGE_FROM: u32 = 3;

/// After `caster` casts `spell`: the first cast of each spell (by name)
/// teaches the player Arcana. Novelty, not stakes: it never inspires.
pub(crate) fn first_cast(state: &mut GameState, caster: Entity, spell: Entity) {
    let Some(core) = state.blueprints.skills().core() else {
        return;
    };
    if caster != state.player || !state.world.satisfies::<&Learner>(caster) {
        return;
    }
    let Ok(name) = state.world.get::<&Name>(spell).map(|name| name.0.clone()) else {
        return;
    };
    let mut attuned = state
        .world
        .get::<&Attuned>(caster)
        .map(|a| (*a).clone())
        .unwrap_or_default();
    if attuned.0.contains(&name) {
        return;
    }
    attuned.0.push(name);
    attuned.0.sort();
    let _ = state.world.insert_one(caster, attuned);
    skill::gain(state, caster, &[(core.arcana, FIRST_CAST_XP)]);
}

/// The spells `creature` knows, as far as the player's Arcana reads them.
fn spells_known(state: &GameState, creature: Entity) -> Vec<String> {
    let world = &state.world;
    let level = state.blueprints.skills().core().map_or(0, |core| {
        skill::level(skill::xp(world, state.player, core.arcana))
    });
    if level < SPELLS_FROM {
        return Vec::new();
    }
    crate::holding::attached(world, creature)
        .into_iter()
        .filter(|&thing| world.satisfies::<&Spell>(thing))
        .map(|spell| {
            let damage = world
                .get::<&SpellDamage>(spell)
                .ok()
                .map(|d| (d.min, d.max));
            match damage {
                Some((min, max)) if level >= SPELL_DAMAGE_FROM => {
                    format!("casts {} ({min}-{max})", noun(world, spell))
                }
                _ => format!("casts {}", noun(world, spell)),
            }
        })
        .collect()
}

/// What the look cursor shows at a tile in view: who or what is there, and
/// as much about a creature as the player's Beast lore reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    pub name: String,
    pub lines: Vec<String>,
}

/// The creature (else the top item) at `at`, if the player can see it.
pub fn inspect(state: &GameState, at: (i32, i32)) -> Option<Inspection> {
    if state.visible.get(at.0, at.1) != Some(&true) {
        return None;
    }
    let world = &state.world;
    let here = |pos: &Position| (pos.x, pos.y) == at;
    let creature = world
        .query::<(hecs::Entity, &Position, &Health)>()
        .without::<&Dead>()
        .iter()
        .find(|(_, pos, _)| here(pos))
        .map(|(entity, _, health)| (entity, *health));
    if let Some((creature, health)) = creature {
        let level = state.blueprints.skills().core().map_or(0, |core| {
            skill::level(skill::xp(world, state.player, core.beast_lore))
        });
        let mut lines = Vec::new();
        // You always know your own health.
        if creature == state.player || level >= HEALTH_FROM {
            lines.push(format!("health {}/{}", health.current, health.max));
        }
        if creature != state.player && level >= ARMOR_FROM {
            let armor = total_armor(world, creature);
            let kinds: Vec<String> = DamageType::ALL
                .iter()
                .filter_map(|&kind| {
                    let value = armor.against(DamageTypes::single(kind));
                    (value != 0).then(|| format!("{} {value}", kind.noun()))
                })
                .collect();
            lines.push(if kinds.is_empty() {
                "no armor".to_string()
            } else {
                format!("armor: {}", kinds.join(", "))
            });
        }
        if creature != state.player && level < HEALTH_FROM {
            lines.push("you know too little of its kind".to_string());
        }
        if creature != state.player {
            lines.extend(spells_known(state, creature));
        }
        return Some(Inspection {
            name: noun(world, creature),
            lines,
        });
    }
    let item = world
        .query::<(hecs::Entity, &Position, &Item, &crate::entity::SpawnSeq)>()
        .iter()
        .filter(|(_, pos, ..)| here(pos))
        .max_by_key(|&(_, _, _, seq)| *seq)
        .map(|(entity, ..)| entity)?;
    Some(Inspection {
        name: noun(world, item),
        lines: Vec::new(),
    })
}

/// Where the look cursor can jump: every other creature in view, nearest
/// first (ties by row, then column), then the player's own tile.
pub fn look_targets(state: &GameState) -> Vec<(i32, i32)> {
    let Ok(me) = state
        .world
        .get::<&Position>(state.player)
        .map(|p| (p.x, p.y))
    else {
        return Vec::new();
    };
    let mut tiles: Vec<(i32, i32)> = state
        .world
        .query::<(hecs::Entity, &Position, &Health)>()
        .without::<&Dead>()
        .iter()
        .filter(|&(entity, pos, _)| {
            entity != state.player && state.visible.get(pos.x, pos.y) == Some(&true)
        })
        .map(|(_, pos, _)| (pos.x, pos.y))
        .collect();
    tiles.sort_by_key(|&(x, y)| (crate::ranged::distance(me, (x, y)), y, x));
    tiles.dedup();
    tiles.push(me);
    tiles
}

fn noun(world: &hecs::World, entity: hecs::Entity) -> String {
    let subject = Subject::of(world, entity);
    subject.noun().trim_start_matches("the ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::entity::{Armor, Skills};
    use crate::state::tests::bare_state;
    use crate::testkit::{see_everything, thing};

    fn lore_xp(state: &GameState) -> u32 {
        let core = state.blueprints.skills().core().unwrap();
        skill::xp(&state.world, state.player, core.beast_lore)
    }

    #[test]
    fn the_first_sight_of_each_kind_teaches_beast_lore_once() {
        let mut state = bare_state();
        let player = state.player;
        state.world.insert_one(player, Learner).unwrap();
        thing("goblin").health(5).at(3, 1).make(&mut state);
        thing("goblin").health(5).at(4, 1).make(&mut state);
        state.run(Command::UpdateFov);
        assert_eq!(lore_xp(&state), FIRST_SIGHT_XP, "two goblins, one kind");
        state.run(Command::UpdateFov);
        assert_eq!(lore_xp(&state), FIRST_SIGHT_XP);
        thing("rat").health(2).at(5, 1).make(&mut state);
        state.run(Command::UpdateFov);
        assert_eq!(lore_xp(&state), 2 * FIRST_SIGHT_XP);
    }

    #[test]
    fn beast_lore_decides_what_a_look_reveals() {
        let mut state = bare_state();
        let goblin = thing("goblin").health(8).at(3, 1).make(&mut state);
        thing("mail")
            .with(Armor {
                slashing: 2,
                ..Armor::default()
            })
            .equipped_on(goblin, 0)
            .make(&mut state);
        see_everything(&mut state);
        let look = |state: &GameState| inspect(state, (3, 1)).unwrap();
        assert_eq!(look(&state).name, "goblin");
        assert_eq!(look(&state).lines, ["you know too little of its kind"]);

        let core = state.blueprints.skills().core().unwrap();
        let mut skills = Skills::default();
        skills.set(core.beast_lore, skill::xp_for(ARMOR_FROM));
        let player = state.player;
        state.world.insert_one(player, skills).unwrap();
        assert_eq!(look(&state).lines, ["health 8/8", "armor: slashing 2"]);

        state.visible.set(3, 1, false);
        assert_eq!(inspect(&state, (3, 1)), None, "only what is in view");
    }

    #[test]
    fn the_first_cast_of_a_spell_teaches_arcana_which_reads_a_casters_spells() {
        let mut state = bare_state();
        let player = state.player;
        state.world.insert_one(player, Learner).unwrap();
        let shaman = thing("shaman").health(9).at(3, 1).make(&mut state);
        thing("blood bolt")
            .spell(crate::entity::School::Hemomancy, 6, 2)
            .spell_damage(2, 5)
            .attached_to(shaman)
            .make(&mut state);
        let spell = thing("firebolt")
            .spell(crate::entity::School::Pyromancy, 6, 1)
            .spell_damage(1, 1)
            .attached_to(player)
            .make(&mut state);
        see_everything(&mut state);
        let core = state.blueprints.skills().core().unwrap();
        let arcana = |state: &GameState| skill::xp(&state.world, state.player, core.arcana);
        let lines = |state: &GameState| inspect(state, (3, 1)).unwrap().lines;
        assert!(!lines(&state).iter().any(|l| l.starts_with("casts")));

        first_cast(&mut state, player, spell);
        first_cast(&mut state, player, spell);
        assert_eq!(arcana(&state), FIRST_CAST_XP, "once per spell");

        let mut skills = Skills::default();
        skills.set(core.arcana, skill::xp_for(SPELLS_FROM));
        state.world.insert_one(player, skills.clone()).unwrap();
        assert!(lines(&state).contains(&"casts blood bolt".to_string()));
        skills.set(core.arcana, skill::xp_for(SPELL_DAMAGE_FROM));
        state.world.insert_one(player, skills).unwrap();
        assert!(lines(&state).contains(&"casts blood bolt (2-5)".to_string()));
    }
}
