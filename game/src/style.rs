//! Styles (ADR 0019): practices a creature discovers and follows. A style
//! grows by uses that follow it; its principles switch on at levels of its
//! skill, so a fresh style changes nothing. Known styles are carried and
//! inert; the followed one is attached.

use std::ops::ControlFlow;

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::blueprint::BlueprintId;
use crate::combat::PerfectDodge;
use crate::command::Command;
use crate::entity::{Does, Principle, Style};
use crate::event::{Ctx, EventRegistry};
use crate::grammar::{Subject, capitalize};
use crate::holding;
use crate::prio;
use crate::skill::{self, Domain, SkillBook, SkillId};
use crate::spawn::Spawn;
use crate::state::GameState;
use crate::technique::{Missing, Names};
use crate::view::ItemRef;

/// A `Style` as written in RON and saves: skills by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleDef {
    pub domains: Vec<Domain>,
    pub skill: String,
    pub follows: Vec<String>,
    /// Skills the wielded weapon must have for a use to follow it (the
    /// short bow); read from the hand, not the use.
    #[serde(default)]
    pub wielding: Vec<String>,
    #[serde(default)]
    pub principles: Vec<Principle>,
    /// Technique blueprints by name, each at a level of its skill.
    #[serde(default)]
    pub invents: Vec<(u32, String)>,
    /// Phases by key, each with % more style XP on a dive there.
    #[serde(default)]
    pub thrives: Vec<(String, u32)>,
}

impl StyleDef {
    /// The skills it names, for checking at load.
    pub(crate) fn names(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.skill)
            .chain(&self.follows)
            .chain(&self.wielding)
    }

    /// With every name resolved, or the first one `names` lacks.
    pub(crate) fn resolve(&self, names: &Names<'_>) -> Result<Style, Missing> {
        let id = |name: &String| {
            names
                .skills
                .id(name)
                .ok_or_else(|| Missing::Skill(name.clone()))
        };
        let mut follows = self.follows.iter().map(id).collect::<Result<Vec<_>, _>>()?;
        follows.sort();
        let mut wielding = self
            .wielding
            .iter()
            .map(id)
            .collect::<Result<Vec<_>, _>>()?;
        wielding.sort();
        Ok(Style {
            domains: self.domains.clone(),
            skill: id(&self.skill)?,
            follows,
            wielding,
            principles: self.principles.clone(),
            invents: self
                .invents
                .iter()
                .map(|(level, name)| {
                    let id =
                        (names.blueprint)(name).ok_or_else(|| Missing::Blueprint(name.clone()))?;
                    Ok((*level, id))
                })
                .collect::<Result<_, Missing>>()?,
            thrives: self.thrives.clone(),
        })
    }

    /// `style` with every id named, or the first one `names` lacks.
    pub(crate) fn of(style: &Style, names: &Names<'_>) -> Result<Self, Missing> {
        let name = |id: SkillId| {
            names
                .skills
                .get(id)
                .map(|def| def.name.clone())
                .ok_or(Missing::SkillId(id))
        };
        Ok(Self {
            domains: style.domains.clone(),
            skill: name(style.skill)?,
            follows: style
                .follows
                .iter()
                .map(|&id| name(id))
                .collect::<Result<_, _>>()?,
            wielding: style
                .wielding
                .iter()
                .map(|&id| name(id))
                .collect::<Result<_, _>>()?,
            principles: style.principles.clone(),
            invents: style
                .invents
                .iter()
                .map(|&(level, id)| {
                    let name = (names.blueprint_name)(id).ok_or(Missing::BlueprintId(id))?;
                    Ok((level, name))
                })
                .collect::<Result<_, Missing>>()?,
            thrives: style.thrives.clone(),
        })
    }
}

/// How much more style XP `skill` (a style's own) gains now, in percent: what
/// the followed style it belongs to thrives on in the dive's phase.
pub(crate) fn thriving(state: &GameState, creature: Entity, skill: SkillId) -> u32 {
    let Some(phase) = crate::phase::on_dive(state) else {
        return 0;
    };
    followed(&state.world, creature)
        .into_iter()
        .filter_map(|style| state.world.get::<&Style>(style).ok().map(|s| (*s).clone()))
        .filter(|style| style.skill == skill)
        .flat_map(|style| style.thrives)
        .filter(|(key, _)| *key == phase.key)
        .map(|(_, pct)| pct)
        .sum()
}

pub(crate) fn register(registry: &mut EventRegistry) {
    registry.on(prio::RIPOSTE, riposte);
}

/// The defender's followed style, at a high enough level, strikes back
/// after a perfect dodge. The blow is queued like any other attack.
fn riposte(
    ctx: &mut Ctx<'_>,
    style: &Style,
    ev: PerfectDodge,
) -> ControlFlow<PerfectDodge, PerfectDodge> {
    let level = skill::level(skill::xp(ctx.world, ev.defender, style.skill));
    let ripostes = style
        .principles
        .iter()
        .any(|p| p.does == Does::Riposte && level >= p.from_level);
    if ripostes {
        ctx.out.push(Command::Attack {
            attacker: ev.defender,
            target: ev.attacker,
        });
    }
    ControlFlow::Continue(ev)
}

/// Each followed style whose skill has reached the level of one of its
/// inventions teaches it, unless `learner` knows it already (ADR 0024).
/// Called only from `skill::train`, after a use with stakes.
pub(crate) fn invent(state: &mut GameState, learner: Entity) {
    let inventions: Vec<(String, BlueprintId)> = followed(&state.world, learner)
        .into_iter()
        .filter_map(|thing| {
            let style = state.world.get::<&Style>(thing).ok()?;
            let level = skill::level(skill::xp(&state.world, learner, style.skill));
            let name = state
                .world
                .get::<&crate::entity::Name>(thing)
                .map_or_else(|_| "your style".to_string(), |name| name.0.clone());
            Some(
                style
                    .invents
                    .iter()
                    .filter(|&&(at, _)| level >= at)
                    .map(|&(_, id)| (name.clone(), id))
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect();
    for (style, id) in inventions {
        let Some(noun) = state.blueprints.by_id(id).and_then(|bp| bp.noun()) else {
            continue;
        };
        let noun = noun.to_string();
        if crate::technique::knows(&state.world, learner, &noun)
            || state.spawn(Spawn::id(id).attached_to(learner)).is_err()
        {
            continue;
        }
        let whom = Subject::of(&state.world, learner);
        state.log.push(capitalize(&format!(
            "{style} teaches {} something new: the {noun}.",
            whom.noun()
        )));
    }
}

/// The next techniques `creature`'s followed styles would invent, within
/// `NEAR` levels: (the style's skill, its level now, the technique's noun).
pub(crate) fn near_inventions(state: &GameState, creature: Entity) -> Vec<(SkillId, u32, String)> {
    const NEAR: u32 = 2;
    let world = &state.world;
    followed(world, creature)
        .into_iter()
        .filter_map(|thing| {
            world
                .get::<&Style>(thing)
                .ok()
                .map(|style| (*style).clone())
        })
        .flat_map(|style| {
            let level = skill::level(skill::xp(world, creature, style.skill));
            style
                .invents
                .iter()
                .filter(|&&(at, _)| level < at && level + NEAR >= at)
                .filter_map(|&(_, id)| state.blueprints.by_id(id).and_then(|bp| bp.noun()))
                .filter(|noun| !crate::technique::knows(world, creature, noun))
                .map(|noun| (style.skill, level, noun.to_string()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The styles `creature` follows (attached), in attachment order.
pub(crate) fn followed(world: &World, creature: Entity) -> Vec<Entity> {
    holding::attached(world, creature)
        .into_iter()
        .filter(|&thing| world.satisfies::<&Style>(thing))
        .collect()
}

/// The styles `creature` knows: followed or not.
pub(crate) fn known(world: &World, creature: Entity) -> Vec<Entity> {
    let mut known = followed(world, creature);
    known.extend(
        holding::carried(world, creature)
            .into_iter()
            .filter(|&thing| world.satisfies::<&Style>(thing)),
    );
    known
}

/// The skills of `creature`'s followed styles that a use of `used` skills
/// follows: they train with it, but never count toward its level. A use
/// follows a style when it is in one of the style's domains, uses all it
/// `follows`, and the wielded weapon has all it asks to be `wielding`.
pub(crate) fn following(
    world: &World,
    book: &SkillBook,
    creature: Entity,
    used: &[SkillId],
) -> Vec<SkillId> {
    let in_hand = || {
        crate::combat::blow_strike(world, book, creature, &Default::default())
            .map_or_else(Vec::new, |(_, skills)| skills)
    };
    followed(world, creature)
        .into_iter()
        .filter_map(|thing| {
            let style = world.get::<&Style>(thing).ok()?;
            let in_domain = used.iter().any(|&skill| {
                book.get(skill)
                    .is_some_and(|def| style.domains.contains(&def.domain))
            });
            let follows = style.follows.iter().all(|skill| used.contains(skill));
            let wielding = style.wielding.is_empty() || {
                let hand = in_hand();
                style.wielding.iter().all(|skill| hand.contains(skill))
            };
            (in_domain && follows && wielding).then_some(style.skill)
        })
        .collect()
}

/// Whether one of `creature`'s followed styles has `does` switched on at
/// its skill's level.
pub(crate) fn principle(world: &World, creature: Entity, does: Does) -> bool {
    followed(world, creature).into_iter().any(|thing| {
        world.get::<&Style>(thing).is_ok_and(|style| {
            let level = skill::level(skill::xp(world, creature, style.skill));
            style
                .principles
                .iter()
                .any(|p| p.does == does && level >= p.from_level)
        })
    })
}

/// Applies `Command::FollowStyle`: sets a followed style aside, or takes up
/// a known one, setting aside any followed style sharing a domain with it.
pub(crate) fn follow(state: &mut GameState, actor: Entity, style: Entity) {
    let name = |state: &GameState, thing| {
        Subject::of(&state.world, thing)
            .noun()
            .trim_start_matches("the ")
            .to_string()
    };
    let who = Subject::of(&state.world, actor);
    if followed(&state.world, actor).contains(&style) {
        set_aside(state, actor, style);
        let message = who.sentence("set", &format!("{} aside", name(state, style)));
        state.log.push(message);
        return;
    }
    if !known(&state.world, actor).contains(&style) {
        return;
    }
    let domains = match state.world.get::<&Style>(style) {
        Ok(taken) => taken.domains.clone(),
        Err(_) => return,
    };
    for other in followed(&state.world, actor) {
        let clashes = state
            .world
            .get::<&Style>(other)
            .is_ok_and(|s| s.domains.iter().any(|d| domains.contains(d)));
        if clashes {
            set_aside(state, actor, other);
        }
    }
    holding::unstow(&mut state.world, actor, style);
    holding::attach(&mut state.world, actor, style);
    let message = who.sentence("take", &format!("up {}", name(state, style)));
    state.log.push(message);
}

fn set_aside(state: &mut GameState, actor: Entity, style: Entity) {
    holding::detach(&mut state.world, actor, style);
    holding::stow(&mut state.world, actor, style);
}

/// One style the player knows, for a skills panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleView {
    /// Pass back as `PlayerAction::FollowStyle`.
    pub style: ItemRef,
    pub name: String,
    pub level: u32,
    pub followed: bool,
}

/// The styles the player knows: followed ones first.
pub fn styles(state: &GameState) -> Vec<StyleView> {
    let world = &state.world;
    let followed = followed(world, state.player);
    known(world, state.player)
        .into_iter()
        .filter_map(|thing| {
            let style = world.get::<&Style>(thing).ok()?;
            Some(StyleView {
                style: ItemRef::of(world, thing)?,
                name: Subject::of(world, thing)
                    .noun()
                    .trim_start_matches("the ")
                    .to_string(),
                level: skill::level(skill::xp(world, state.player, style.skill)),
                followed: followed.contains(&thing),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::blueprint::Blueprints;
    use crate::command::dispatch;
    use crate::entity::{Health, Learner, Skills};
    use crate::input::PlayerAction;
    use crate::spawn::Spawn;
    use crate::state::tests::bare_state_with;
    use crate::testkit::{last_log, sure_hits, thing};
    use crate::time::take_turn;
    use crate::view::inventory_view;

    fn data() -> Blueprints {
        Blueprints::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")).unwrap()
    }

    /// The player as a learner at these levels, with a rapier in hand and the
    /// other hand free.
    fn fencer(levels: &[(&str, u32)]) -> GameState {
        let mut state = bare_state_with(data());
        let player = state.player;
        let mut skills = Skills::default();
        for &(name, level) in levels {
            let id = state.blueprints.skills().id(name).unwrap();
            skills.set(id, skill::xp_for(level));
        }
        state.world.insert(player, (Learner, skills)).unwrap();
        // The player starts with a bow and a torch in hand; put them away.
        for name in ["short bow", "torch"] {
            let held = crate::view::equipment_view(&state)
                .into_iter()
                .find_map(|part| part.item.filter(|item| item.name == name))
                .unwrap()
                .item;
            take_turn(&mut state, PlayerAction::Unequip(held));
        }
        let rapier = state.blueprints.id("Rapier").unwrap();
        state.spawn(Spawn::id(rapier).carried_by(player)).unwrap();
        let rapier = inventory_view(&state)
            .into_iter()
            .find(|item| item.name == "rapier")
            .unwrap()
            .item;
        take_turn(
            &mut state,
            PlayerAction::Equip {
                item: rapier,
                part: None,
            },
        );
        state
    }

    fn goblin(state: &mut GameState) -> Entity {
        thing("goblin")
            .health(1000)
            .melee(1, 1)
            .hostile()
            .at(2, 1)
            .make(state)
    }

    fn hit(state: &mut GameState, target: Entity) {
        let player = state.player;
        state.run(Command::Attack {
            attacker: player,
            target,
        });
    }

    fn fencing_xp(state: &GameState) -> u32 {
        let id = state.blueprints.skills().id("Fencing").unwrap();
        skill::xp(&state.world, state.player, id)
    }

    fn take_up(state: &mut GameState, name: &str) {
        let style = styles(state)
            .into_iter()
            .find(|s| s.name == name)
            .unwrap()
            .style;
        take_turn(state, PlayerAction::FollowStyle(style));
    }

    /// Fencing, discovered by hitting a goblin at the threshold.
    fn discovered() -> (GameState, Entity) {
        let mut state = fencer(&[
            ("Melee", 3),
            ("Pierce", 3),
            ("One-handed", 3),
            ("Empty offhand", 3),
        ]);
        let player = state.player;
        sure_hits(&mut state, player);
        let goblin = goblin(&mut state);
        hit(&mut state, goblin);
        (state, goblin)
    }

    #[test]
    fn a_discovered_style_is_known_not_followed_and_not_an_item() {
        let (state, _) = discovered();
        assert!(
            state
                .log
                .recent(5)
                .any(|l| l == "Inspiration! You discover Fencing.")
        );
        let known = styles(&state);
        assert_eq!(known.len(), 1);
        assert_eq!(
            (known[0].name.as_str(), known[0].followed),
            ("fencing", false)
        );
        assert!(
            inventory_view(&state)
                .iter()
                .all(|item| item.name != "fencing")
        );
    }

    #[test]
    fn following_takes_a_turn_and_toggles() {
        let (mut state, _) = discovered();
        let now = state.now;
        take_up(&mut state, "fencing");
        assert!(state.now > now, "it took a turn");
        assert_eq!(last_log(&state), "You take up fencing.");
        assert!(styles(&state)[0].followed);
        take_up(&mut state, "fencing");
        assert_eq!(last_log(&state), "You set fencing aside.");
        assert!(!styles(&state)[0].followed);
    }

    #[test]
    fn taking_up_a_style_sets_aside_one_sharing_a_domain() {
        let (mut state, _) = discovered();
        let player = state.player;
        let fencing = state.blueprints.id("Fencing").unwrap();
        let mut other = Spawn::id(fencing).carried_by(player);
        other = other.with(crate::entity::Name("brawling".into()));
        state.spawn(other).unwrap();
        take_up(&mut state, "fencing");
        take_up(&mut state, "brawling");
        let followed: Vec<(String, bool)> = styles(&state)
            .into_iter()
            .map(|s| (s.name, s.followed))
            .collect();
        assert_eq!(
            followed,
            [("brawling".into(), true), ("fencing".into(), false)]
        );
    }

    #[test]
    fn a_followed_style_trains_only_with_uses_that_follow_it_and_never_weakens_a_blow() {
        let (mut state, goblin) = discovered();
        hit(&mut state, goblin);
        assert_eq!(fencing_xp(&state), 0, "known but not followed");

        let player = state.player;
        let level = |state: &GameState| {
            crate::combat::strike(&state.world, state.blueprints.skills(), player)
                .unwrap()
                .level
        };
        let before = level(&state);
        take_up(&mut state, "fencing");
        assert_eq!(level(&state), before, "a fresh style changes nothing");
        hit(&mut state, goblin);
        let trained = fencing_xp(&state);
        assert!(trained > 0);

        // Something in the other hand: no longer fencing.
        thing("buckler").equipped_on(player, 4).make(&mut state);
        hit(&mut state, goblin);
        assert_eq!(fencing_xp(&state), trained);
    }

    #[test]
    fn at_its_level_fencing_ripostes_a_perfect_dodge() {
        let (mut state, goblin) = discovered();
        take_up(&mut state, "fencing");
        let player = state.player;
        let ev = PerfectDodge {
            attacker: goblin,
            defender: player,
        };
        let health = |state: &GameState| state.world.get::<&Health>(goblin).unwrap().current;
        let before = health(&state);
        let _ = dispatch(&mut state, player, ev);
        crate::command::apply_commands(&mut state);
        assert_eq!(health(&state), before, "below level 5: nothing yet");

        let fencing = state.blueprints.skills().id("Fencing").unwrap();
        state
            .world
            .get::<&mut Skills>(player)
            .unwrap()
            .set(fencing, skill::xp_for(5));
        let _ = dispatch(&mut state, player, ev);
        crate::command::apply_commands(&mut state);
        assert!(health(&state) < before, "the riposte landed");
    }
}
