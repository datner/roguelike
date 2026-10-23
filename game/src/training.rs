//! Training on purpose (ADR 0017): lessons, a big chunk of XP once per
//! kind of thing, and practice, a little XP a turn up to a ceiling. Neither
//! has stakes, so neither repeats familiarity, trains a style or inspires.

use hecs::Entity;

use crate::blueprint::{BlueprintId, ResolvedBlueprint};
use crate::combat::strike;
use crate::entity::{Learner, Lesson, Name, Studied, Teaches};
use crate::explore::visible_hostile;
use crate::grammar::Subject;
use crate::input::PlayerAction;
use crate::skill::{self, SkillId};
use crate::spawn::Spawn;
use crate::state::GameState;
use crate::time::take_turn;

/// XP a turn of practice gives each skill it can still raise.
pub(crate) const PRACTICE_XP: u32 = 2;
/// The level practice reaches in a domain nothing has been learned in yet.
const PRACTICE_FLOOR: u32 = 2;

/// Applies `Command::Study`: `actor` learns `item`'s lesson, unless they
/// already studied that kind of thing.
pub(crate) fn study(state: &mut GameState, actor: Entity, item: Entity) {
    let who = Subject::of(&state.world, actor);
    let what = Subject::of(&state.world, item);
    let lesson = state.world.get::<&Lesson>(item).ok().map(|l| l.0.clone());
    let teaches = state.world.get::<&Teaches>(item).ok().map(|t| t.0.clone());
    if lesson.is_none() && teaches.is_none() {
        state
            .log
            .push(who.sentence("find", &format!("nothing to learn in {}", what.noun())));
        return;
    }
    if !state.world.satisfies::<&Learner>(actor) {
        return;
    }
    let kind = state
        .world
        .get::<&Name>(item)
        .map_or_else(|_| "thing".to_string(), |name| name.0.clone());
    let mut studied = state
        .world
        .get::<&Studied>(actor)
        .map(|s| (*s).clone())
        .unwrap_or_default();
    if studied.0.contains(&kind) {
        state.log.push(who.sentence(
            "have",
            &format!("nothing more to learn from {}", what.noun()),
        ));
        return;
    }
    studied.0.push(kind);
    studied.0.sort();
    let _ = state.world.insert_one(actor, studied);
    state.log.push(who.sentence("study", what.noun()));
    if let Some(lesson) = lesson {
        skill::gain(state, actor, &lesson);
    }
    for technique in teaches.into_iter().flatten() {
        learn(state, actor, technique);
    }
}

/// `learner` comes to know the technique `blueprint` (ADR 0024), unless it
/// already knows one of that name.
fn learn(state: &mut GameState, learner: Entity, blueprint: BlueprintId) {
    let noun = state
        .blueprints
        .by_id(blueprint)
        .and_then(ResolvedBlueprint::noun)
        .unwrap_or("technique")
        .to_string();
    if crate::technique::knows(&state.world, learner, &noun)
        || state
            .spawn(Spawn::id(blueprint).attached_to(learner))
            .is_err()
    {
        return;
    }
    let message = Subject::of(&state.world, learner).sentence("learn", &format!("the {noun}"));
    state.log.push(message);
}

/// How high practice can take `skill`: half the learner's best level in its
/// domain, and never below `PRACTICE_FLOOR`. Practice tops up what real use
/// has reached; it never replaces it.
fn ceiling(state: &GameState, learner: Entity, skill: SkillId) -> u32 {
    let book = state.blueprints.skills();
    let Some(domain) = book.get(skill).map(|def| def.domain) else {
        return 0;
    };
    let best = state
        .world
        .get::<&crate::entity::Skills>(learner)
        .map_or(0, |learned| {
            learned
                .0
                .iter()
                .filter(|&&(id, _)| book.get(id).is_some_and(|def| def.domain == domain))
                .map(|&(_, xp)| skill::level(xp))
                .max()
                .unwrap_or(0)
        });
    (best / 2).max(PRACTICE_FLOOR)
}

/// The skills drilling with what `learner` wields would still raise.
fn drillable(state: &GameState, learner: Entity) -> Vec<SkillId> {
    let book = state.blueprints.skills();
    let Some(strike) = strike(&state.world, book, learner) else {
        return Vec::new();
    };
    skill::melee(&state.world, book, learner, &strike)
        .into_iter()
        .filter(|&id| {
            skill::level(skill::xp(&state.world, learner, id)) < ceiling(state, learner, id)
        })
        .collect()
}

/// Applies `Command::Practice`: a turn of drills with the wielded weapon
/// (or bare hands), a little XP to each skill below its ceiling.
pub(crate) fn practice(state: &mut GameState, actor: Entity) {
    if !state.world.satisfies::<&Learner>(actor) {
        return;
    }
    let gains: Vec<(SkillId, u32)> = drillable(state, actor)
        .into_iter()
        .map(|id| (id, PRACTICE_XP))
        .collect();
    skill::gain(state, actor, &gains);
}

/// Takes one turn of practice as the player's and returns whether to keep
/// going, like `explore_step`. Stops without spending time when an enemy is
/// in view or there is nothing left to learn by drilling (the log says why).
pub fn practice_step(state: &mut GameState) -> bool {
    if state.game_over {
        return false;
    }
    if let Some(hostile) = visible_hostile(state) {
        state
            .log
            .push(Subject::of(&state.world, hostile).sentence("be", "in view"));
        return false;
    }
    if drillable(state, state.player).is_empty() {
        state.log.push("You can learn no more by drilling alone.");
        return false;
    }
    take_turn(state, PlayerAction::Practice);
    !state.game_over
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::blueprint::Blueprints;
    use crate::entity::Skills;
    use crate::spawn::Spawn;
    use crate::state::tests::bare_state_with;
    use crate::testkit::{last_log, see_everything, thing};
    use crate::view::{ItemRef, inventory_view};

    fn data() -> Blueprints {
        Blueprints::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")).unwrap()
    }

    fn learner(levels: &[(&str, u32)]) -> GameState {
        let mut state = bare_state_with(data());
        let player = state.player;
        let mut skills = Skills::default();
        for &(name, level) in levels {
            let id = state.blueprints.skills().id(name).unwrap();
            skills.set(id, skill::xp_for(level));
        }
        state.world.insert(player, (Learner, skills)).unwrap();
        state
    }

    fn xp(state: &GameState, name: &str) -> u32 {
        let id = state.blueprints.skills().id(name).unwrap();
        skill::xp(&state.world, state.player, id)
    }

    fn study_a_manual(state: &mut GameState) {
        let manual = state.blueprints.id("FencingManual").unwrap();
        let player = state.player;
        state.spawn(Spawn::id(manual).carried_by(player)).unwrap();
        let item: ItemRef = inventory_view(state)
            .into_iter()
            .find(|item| item.lesson)
            .unwrap()
            .item;
        take_turn(state, PlayerAction::Study(item));
    }

    #[test]
    fn a_lesson_teaches_once_per_kind_and_never_inspires() {
        // One lesson short of discovering Fencing.
        let mut state = learner(&[("Melee", 3), ("One-handed", 3), ("Empty offhand", 3)]);
        let now = state.now;
        study_a_manual(&mut state);
        assert!(state.now > now, "studying takes time");
        assert_eq!((xp(&state, "Sword"), xp(&state, "Pierce")), (200, 100));
        assert!(
            state
                .log
                .recent(5)
                .any(|l| l == "You study the fencing manual.")
        );
        assert!(!state.log.recent(5).any(|l| l.starts_with("Inspiration")));

        study_a_manual(&mut state);
        assert_eq!(
            last_log(&state),
            "You have nothing more to learn from the fencing manual."
        );
        assert_eq!(xp(&state, "Sword"), 200);
    }

    #[test]
    fn practice_drills_up_to_half_the_best_level_in_the_domain() {
        let mut state = learner(&[("Melee", 8)]);
        see_everything(&mut state);
        // The player starts with a bow, and drills strike with it: Melee, Bow
        // and Blunt; Melee is past its ceiling of 4 already.
        let mut turns = 0;
        while practice_step(&mut state) {
            turns += 1;
            assert!(turns < 10_000, "practice never stopped");
        }
        assert_eq!(last_log(&state), "You can learn no more by drilling alone.");
        for name in ["Bow", "Blunt"] {
            assert_eq!(skill::level(xp(&state, name)), 4, "{name}");
        }
        assert_eq!(xp(&state, "Melee"), skill::xp_for(8));
    }

    #[test]
    fn practice_starts_from_a_floor_and_stops_for_an_enemy_in_view() {
        let mut state = learner(&[]);
        see_everything(&mut state);
        while practice_step(&mut state) {}
        assert_eq!(skill::level(xp(&state, "Bow")), PRACTICE_FLOOR);

        let mut state = learner(&[]);
        thing("goblin")
            .hostile()
            .health(5)
            .at(3, 1)
            .make(&mut state);
        see_everything(&mut state);
        let now = state.now;
        assert!(!practice_step(&mut state));
        assert_eq!(last_log(&state), "The goblin is in view.");
        assert_eq!(state.now, now);
    }
}
