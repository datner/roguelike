//! Skills (ADR 0017): how well a creature does what it does, grown by doing
//! it. Skills are flat, overlapping tags listed in `data/skills.ron`; one
//! action uses several at once, and its level is their weighted average.
//! Only XP is stored; levels are derived, so the curve can change freely.

use std::collections::BTreeMap;
use std::path::Path;

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::blueprint::BlueprintError;
use crate::combat::{AttackOdds, CERTAIN, Outcome, Strike};
use crate::entity::{
    Area, Body, Chain, DamageType, Delay, EquippedOn, Familiarity, Learner, Name, PartKind, School,
    Shape, Skills, Spell, SpellDamage, Summon, Trains, Weapon,
};
use crate::equipment::equipped;
use crate::grammar::{Subject, capitalize};
use crate::state::GameState;

/// A skill's index in the `SkillBook` it was loaded into. Runtime-only, like
/// `BlueprintId`: anything persisted stores the skill's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SkillId(u16);

/// What area of life a skill belongs to. A style fills one slot per domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Domain {
    Martial,
    Defence,
    Magic,
    Lore,
    /// Being at home in a phase (docs/plans/phases.md).
    Attunement,
    /// Making things (docs/plans/crafting.md): never counts toward a fight.
    Craft,
}

/// One entry of `data/skills.ron`. `weight` is how much the skill counts in
/// an action's level: specific skills weigh more than general ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDef {
    pub name: String,
    pub domain: Domain,
    pub weight: u32,
}

/// The skills the code names itself, because an action or a circumstance
/// implies them rather than a part. Resolved once at load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Core {
    pub melee: SkillId,
    pub unarmed: SkillId,
    pub pierce: SkillId,
    pub slash: SkillId,
    pub blunt: SkillId,
    pub dodge: SkillId,
    pub poise: SkillId,
    pub deflection: SkillId,
    pub one_handed: SkillId,
    pub empty_offhand: SkillId,
    pub thrown: SkillId,
    pub archery: SkillId,
    pub magic: SkillId,
    pub pyromancy: SkillId,
    pub arcane: SkillId,
    pub hemomancy: SkillId,
    pub bolt: SkillId,
    pub burst: SkillId,
    pub cone: SkillId,
    pub beam: SkillId,
    pub nova: SkillId,
    pub chain: SkillId,
    pub rune: SkillId,
    pub summoning: SkillId,
    pub beast_lore: SkillId,
    pub toughness: SkillId,
    pub recovery: SkillId,
    pub channeling: SkillId,
    pub bloodletting: SkillId,
    pub scroll_lore: SkillId,
    pub arcana: SkillId,
    /// What a kill yields (docs/plans/crafting.md).
    pub scavenging: SkillId,
}

impl Core {
    /// The skill trained by striking with `kind`, if any.
    fn of_damage(self, kind: DamageType) -> Option<SkillId> {
        match kind {
            DamageType::Piercing => Some(self.pierce),
            DamageType::Slashing => Some(self.slash),
            DamageType::Blunt => Some(self.blunt),
            DamageType::Fire | DamageType::Cold | DamageType::Shock | DamageType::Acid => None,
        }
    }
}

/// Every skill there is. Empty by default, for tests that don't care about
/// skills: without the core skills no action uses or trains any.
#[derive(Debug, Clone, Default)]
pub struct SkillBook {
    /// Indexed by `SkillId`; ids follow file order.
    defs: Vec<SkillDef>,
    ids: BTreeMap<String, SkillId>,
    core: Option<Core>,
}

impl SkillBook {
    /// Reads `data/skills.ron`: a list of `SkillDef`s.
    pub fn load(path: &Path) -> Result<Self, BlueprintError> {
        let text =
            std::fs::read_to_string(path).map_err(|err| BlueprintError::Io(path.into(), err))?;
        let defs = ron::from_str(&text)
            .map_err(|err| BlueprintError::Parse(path.into(), Box::new(err)))?;
        Self::new(defs)
    }

    /// Rejects a name listed twice and a missing core skill, so neither can
    /// surface during play.
    pub fn new(defs: Vec<SkillDef>) -> Result<Self, BlueprintError> {
        let mut ids = BTreeMap::new();
        for (index, def) in defs.iter().enumerate() {
            let id = u16::try_from(index)
                .map(SkillId)
                .map_err(|_| BlueprintError::TooManySkills)?;
            if ids.insert(def.name.clone(), id).is_some() {
                return Err(BlueprintError::DuplicateSkill(def.name.clone()));
            }
        }
        let core_id = |name: &str| {
            ids.get(name)
                .copied()
                .ok_or_else(|| BlueprintError::MissingSkill(name.to_string()))
        };
        let core = Core {
            melee: core_id("Melee")?,
            unarmed: core_id("Unarmed")?,
            pierce: core_id("Pierce")?,
            slash: core_id("Slash")?,
            blunt: core_id("Blunt")?,
            dodge: core_id("Dodge")?,
            poise: core_id("Poise")?,
            deflection: core_id("Deflection")?,
            one_handed: core_id("One-handed")?,
            empty_offhand: core_id("Empty offhand")?,
            thrown: core_id("Thrown")?,
            archery: core_id("Archery")?,
            magic: core_id("Magic")?,
            pyromancy: core_id("Pyromancy")?,
            arcane: core_id("Arcane")?,
            hemomancy: core_id("Hemomancy")?,
            bolt: core_id("Bolt")?,
            burst: core_id("Burst")?,
            cone: core_id("Cone")?,
            beam: core_id("Beam")?,
            nova: core_id("Nova")?,
            chain: core_id("Chain")?,
            rune: core_id("Rune")?,
            summoning: core_id("Summoning")?,
            beast_lore: core_id("Beast lore")?,
            toughness: core_id("Toughness")?,
            recovery: core_id("Recovery")?,
            channeling: core_id("Channeling")?,
            bloodletting: core_id("Bloodletting")?,
            scroll_lore: core_id("Scroll lore")?,
            arcana: core_id("Arcana")?,
            scavenging: core_id("Scavenging")?,
        };
        Ok(Self {
            defs,
            ids,
            core: Some(core),
        })
    }

    /// The skills the code names, if the book has them (not an empty one).
    pub(crate) fn core(&self) -> Option<Core> {
        self.core
    }

    pub fn id(&self, name: &str) -> Option<SkillId> {
        self.ids.get(name).copied()
    }

    pub fn get(&self, id: SkillId) -> Option<&SkillDef> {
        self.defs.get(usize::from(id.0))
    }

    fn weight(&self, id: SkillId) -> u32 {
        self.get(id).map_or(0, |def| def.weight)
    }
}

/// The highest level there is.
pub const MAX_LEVEL: u32 = 20;
/// XP for one counted use, before repetition decays it.
pub(crate) const BASE_XP: u32 = 10;
/// A use's XP divided by this is what it attunes to the dive's phase.
const ATTUNE_SHARE: u32 = 4;
/// How fast repetition decays XP: the `n`th use against the same kind of
/// thing is worth `DECAY / (DECAY + n)` of a first one.
const DECAY: u32 = 20;

/// Total XP needed to reach `level`.
pub fn xp_for(level: u32) -> u32 {
    50 * level * level
}

/// The level `xp` reaches.
pub fn level(xp: u32) -> u32 {
    (0..=MAX_LEVEL)
        .take_while(|&n| xp_for(n) <= xp)
        .last()
        .unwrap_or(0)
}

/// `creature`'s XP in `skill`.
pub(crate) fn xp(world: &World, creature: Entity, skill: SkillId) -> u32 {
    world.get::<&Skills>(creature).map_or(0, |skills| {
        skills
            .0
            .iter()
            .find(|&&(id, _)| id == skill)
            .map_or(0, |&(_, xp)| xp)
    })
}

/// `creature`'s level at an action using `skills`: the weighted average of
/// its levels in them, rounded down. A sum would make the tool with the most
/// skills the best one; an average keeps extra skills neutral.
pub(crate) fn effective(
    world: &World,
    book: &SkillBook,
    creature: Entity,
    skills: &[SkillId],
) -> u32 {
    let total: u32 = skills.iter().map(|&id| book.weight(id)).sum();
    if total == 0 {
        return 0;
    }
    let weighted: u32 = skills
        .iter()
        .map(|&id| book.weight(id) * level(xp(world, creature, id)))
        .sum();
    weighted / total
}

/// Extra damage at an effective level.
pub(crate) fn damage_bonus(level: u32) -> i32 {
    i32::try_from(level / 4).unwrap_or(0)
}

/// The skills a melee `strike` uses: Melee, what the weapon trains (Unarmed
/// for natural attacks), one skill per kind of damage it deals, and how it
/// is held: One-handed for a weapon in one hand, Empty offhand when every
/// other hand is free. Sorted and without duplicates.
pub(crate) fn melee(
    world: &World,
    book: &SkillBook,
    attacker: Entity,
    strike: &Strike,
) -> Vec<SkillId> {
    let Some(core) = book.core else {
        return Vec::new();
    };
    let mut skills = vec![core.melee];
    match strike.weapon {
        Some(weapon) => {
            if let Ok(trains) = world.get::<&Trains>(weapon) {
                skills.extend(trains.0.iter().copied());
            }
            skills.extend(grip(world, core, attacker, weapon));
        }
        None => skills.push(core.unarmed),
    }
    skills.extend(strike.types.iter().filter_map(|kind| core.of_damage(kind)));
    skills.sort();
    skills.dedup();
    skills
}

/// How `weapon` is held by whoever wields it. Every weapon takes one hand
/// for now (no two-handed weapons yet); the off hand is empty when the
/// wielder has another hand and nothing is equipped on any of them.
fn grip(world: &World, core: Core, wielder: Entity, weapon: Entity) -> Vec<SkillId> {
    let Ok(on) = world.get::<&EquippedOn>(weapon).map(|on| on.0) else {
        return Vec::new();
    };
    let mut grip = vec![core.one_handed];
    let hands: Vec<u32> = world
        .get::<&Body>(wielder)
        .map(|body| {
            (0..)
                .zip(&body.0)
                .filter(|&(index, part)| part.kind == PartKind::Hand && index != on)
                .map(|(index, _)| index)
                .collect()
        })
        .unwrap_or_default();
    let worn = equipped(world, wielder);
    let free = |hand: &u32| !worn.iter().any(|&(_, part)| part == *hand);
    if !hands.is_empty() && hands.iter().all(free) {
        grip.push(core.empty_offhand);
    }
    grip
}

/// How a missile leaves its shooter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Launch {
    Thrown,
    /// From this launcher.
    Fired(Entity),
}

/// The skills a missile `strike` uses: Thrown and what the missile trains,
/// or Archery and what the launcher trains (a bow, Bow); then one skill per
/// kind of damage it deals. Sorted and without duplicates.
pub(crate) fn missile(
    world: &World,
    book: &SkillBook,
    launch: Launch,
    strike: &Strike,
) -> Vec<SkillId> {
    let Some(core) = book.core else {
        return Vec::new();
    };
    let (action, tool) = match launch {
        Launch::Thrown => (core.thrown, strike.weapon),
        Launch::Fired(launcher) => (core.archery, Some(launcher)),
    };
    let mut skills = vec![action];
    if let Some(Ok(trains)) = tool.map(|tool| world.get::<&Trains>(tool)) {
        skills.extend(trains.0.iter().copied());
    }
    skills.extend(strike.types.iter().filter_map(|kind| core.of_damage(kind)));
    skills.sort();
    skills.dedup();
    skills
}

/// The skills casting `spell` uses: Magic, its school, its form (Bolt for
/// damage without an area, else the area's shape; Chain, Rune and
/// Summoning on top for those payloads) and whatever it `Trains`. A scroll
/// is its spell, so reading one trains the same. Sorted, without duplicates.
pub(crate) fn cast(world: &World, book: &SkillBook, spell: Entity) -> Vec<SkillId> {
    let (Some(core), Ok(data)) = (book.core, world.get::<&Spell>(spell)) else {
        return Vec::new();
    };
    let school = match data.school {
        School::Pyromancy => core.pyromancy,
        School::Arcane => core.arcane,
        School::Hemomancy => core.hemomancy,
    };
    let mut skills = vec![core.magic, school];
    match world.get::<&Area>(spell).map(|area| area.shape) {
        Ok(Shape::Burst { .. }) => skills.push(core.burst),
        Ok(Shape::Cone) => skills.push(core.cone),
        Ok(Shape::Beam) => skills.push(core.beam),
        Ok(Shape::Nova { .. }) => skills.push(core.nova),
        Err(_) if world.satisfies::<&SpellDamage>(spell) => skills.push(core.bolt),
        Err(_) => {}
    }
    for (payload, skill) in [
        (world.satisfies::<&Chain>(spell), core.chain),
        (world.satisfies::<&Delay>(spell), core.rune),
        (world.satisfies::<&Summon>(spell), core.summoning),
    ] {
        if payload {
            skills.push(skill);
        }
    }
    if let Ok(trains) = world.get::<&Trains>(spell) {
        skills.extend(trains.0.iter().copied());
    }
    skills.sort();
    skills.dedup();
    skills
}

/// How much less likely a cast is to fizzle per level of skill at it.
const FIZZLE_BP_PER_LEVEL: u32 = 200;

/// `fizzle_bp` for a caster at `level` with the spell: steadier hands.
pub(crate) fn steadied(fizzle_bp: u32, level: u32) -> u32 {
    fizzle_bp.saturating_sub(FIZZLE_BP_PER_LEVEL * level)
}

/// The skills wielding `item` trains beyond the action's own: what it
/// `Trains`, and one per kind of damage its `Weapon` deals. For showing on
/// the item; `melee` decides for a real strike.
pub(crate) fn of_item(world: &World, book: &SkillBook, item: Entity) -> Vec<SkillId> {
    let mut skills = world
        .get::<&Trains>(item)
        .map(|trains| trains.0.clone())
        .unwrap_or_default();
    if let (Some(core), Ok(weapon)) = (book.core, world.get::<&Weapon>(item)) {
        skills.extend(weapon.types.iter().filter_map(|kind| core.of_damage(kind)));
    }
    skills.sort();
    skills.dedup();
    skills
}

/// `learner` used `skills` against `subject`: each gains `base` XP, less the
/// more often it has been used against that kind of thing (by name). Logs
/// each level gained. Does nothing for creatures without `Learner`.
pub(crate) fn train(
    state: &mut GameState,
    learner: Entity,
    skills: &[SkillId],
    subject: Entity,
    base: u32,
) {
    if !state.world.satisfies::<&Learner>(learner) {
        return;
    }
    grow(state, learner, skills, subject, base);
    // Only uses with stakes reach here, so only they can invent or inspire.
    crate::style::invent(state, learner);
    crate::discovery::inspire(state, learner);
}

/// `train` without its moment of inspiration: a use that counts, decaying
/// with repetition against the same kind of thing, but whose stakes don't
/// call for one (a craft from common materials, docs/plans/crafting.md).
pub(crate) fn grow(
    state: &mut GameState,
    learner: Entity,
    skills: &[SkillId],
    subject: Entity,
    base: u32,
) {
    if !state.world.satisfies::<&Learner>(learner) {
        return;
    }
    // A followed style trains along with any use that follows it.
    let mut skills = skills.to_vec();
    let book = state.blueprints.skills();
    skills.extend(crate::style::following(
        &state.world,
        book,
        learner,
        &skills,
    ));
    skills.sort();
    skills.dedup();
    let kind = state
        .world
        .get::<&Name>(subject)
        .map_or_else(|_| "something".to_string(), |name| name.0.clone());
    let mut gains = Vec::with_capacity(skills.len());
    {
        let mut familiarity = state
            .world
            .get::<&Familiarity>(learner)
            .map(|f| (*f).clone())
            .unwrap_or_default();
        for &skill in &skills {
            let uses = familiarity.uses(skill, &kind);
            // A followed style thriving in the dive's phase grows faster.
            let more = crate::style::thriving(state, learner, skill);
            gains.push((skill, base * DECAY / (DECAY + uses) * (100 + more) / 100));
            familiarity.count(skill, &kind);
        }
        let _ = state.world.insert_one(learner, familiarity);
    }
    // The dive's phase attunes every use (docs/plans/phases.md), a share of
    // it: it rides along with everything, so at full XP it would outgrow
    // the skills that earned it.
    if let Some(attunement) = crate::phase::attuning(state) {
        gains.push((attunement, base / ATTUNE_SHARE));
    }
    gain(state, learner, &gains);
}

/// Adds each `(skill, xp)` to `learner`'s skills and logs every level
/// gained. The bare gain: no repetition, no styles, no inspiration. Lessons
/// and practice use it directly; `train` adds those for uses with stakes.
pub(crate) fn gain(state: &mut GameState, learner: Entity, gains: &[(SkillId, u32)]) {
    let mut learned = state
        .world
        .get::<&Skills>(learner)
        .map(|s| (*s).clone())
        .unwrap_or_default();
    let mut risen = Vec::new();
    for &(skill, gain) in gains {
        let before = learned.xp(skill);
        let after = before.saturating_add(gain);
        learned.set(skill, after);
        if level(after) > level(before) {
            risen.push((skill, level(after)));
        }
    }
    let _ = state.world.insert_one(learner, learned);

    let whose = Subject::of(&state.world, learner).possessive();
    for (skill, level) in risen {
        let Some(def) = state.blueprints.skills().get(skill) else {
            continue;
        };
        let name = def.name.to_lowercase();
        state.log.push(capitalize(&format!(
            "{whose} {name} skill rises to {level}."
        )));
    }
}

/// After a blow between enemies, both sides learn from it. The attacker
/// trains the skills it `used`: a hit teaches more the less likely it was, a
/// miss or graze teaches half. A learning defender trains Dodge and Poise
/// (and Deflection if it wore something to deflect with); a perfect dodge
/// teaches up to double, more the less likely it was. `extra_pct` adds to
/// what the attacker learns (a run of a form, ADR 0024).
pub(crate) fn after_blow(
    state: &mut GameState,
    used: &[SkillId],
    odds: &AttackOdds,
    outcome: Outcome,
    extra_pct: u32,
) {
    let unlikely = |bp: u32| BASE_XP * (2 * CERTAIN - bp.min(CERTAIN)) / CERTAIN;
    let xp = match outcome {
        Outcome::Hit => unlikely(odds.hit_bp),
        _ => BASE_XP / 2,
    };
    let xp = xp * (100 + extra_pct) / 100;
    train(state, odds.attacker, used, odds.defender, xp);

    let Some(core) = state.blueprints.skills().core() else {
        return;
    };
    let mut guard = vec![core.dodge, core.poise];
    if odds.deflector.is_some() {
        guard.push(core.deflection);
    }
    guard.sort();
    let xp = match outcome {
        Outcome::PerfectDodge => unlikely(odds.perfect_bp),
        _ => BASE_XP,
    };
    train(state, odds.defender, &guard, odds.attacker, xp);
}

impl Skills {
    pub(crate) fn xp(&self, skill: SkillId) -> u32 {
        self.0
            .iter()
            .find(|&&(id, _)| id == skill)
            .map_or(0, |&(_, xp)| xp)
    }

    /// Sets `skill`'s XP, keeping the list sorted by id.
    pub(crate) fn set(&mut self, skill: SkillId, xp: u32) {
        match self.0.binary_search_by_key(&skill, |&(id, _)| id) {
            Ok(at) => self.0[at].1 = xp,
            Err(at) => self.0.insert(at, (skill, xp)),
        }
    }
}

impl Familiarity {
    fn uses(&self, skill: SkillId, kind: &str) -> u32 {
        self.0
            .iter()
            .find(|(id, name, _)| *id == skill && name == kind)
            .map_or(0, |&(_, _, uses)| uses)
    }

    /// One more use of `skill` against `kind`, keeping the list sorted.
    fn count(&mut self, skill: SkillId, kind: &str) {
        match self
            .0
            .binary_search_by(|(id, name, _)| (*id, name.as_str()).cmp(&(skill, kind)))
        {
            Ok(at) => self.0[at].2 += 1,
            Err(at) => self.0.insert(at, (skill, kind.to_string(), 1)),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::command::Command;
    use crate::entity::{Health, Stat};
    use crate::state::tests::bare_state;
    use crate::testkit::{sure_hits, thing};

    fn def(name: &str, domain: Domain, weight: u32) -> SkillDef {
        SkillDef {
            name: name.to_string(),
            domain,
            weight,
        }
    }

    /// The core skills plus Dagger, weighted as in `data/skills.ron`.
    pub(crate) fn book() -> SkillBook {
        SkillBook::new(vec![
            def("Melee", Domain::Martial, 1),
            def("Unarmed", Domain::Martial, 3),
            def("Pierce", Domain::Martial, 2),
            def("Slash", Domain::Martial, 2),
            def("Blunt", Domain::Martial, 2),
            def("Dagger", Domain::Martial, 3),
            def("Dodge", Domain::Defence, 1),
            def("Poise", Domain::Defence, 1),
            def("Deflection", Domain::Defence, 1),
            def("Toughness", Domain::Defence, 1),
            def("Recovery", Domain::Defence, 1),
            def("One-handed", Domain::Martial, 1),
            def("Empty offhand", Domain::Martial, 1),
            def("Thrown", Domain::Martial, 1),
            def("Archery", Domain::Martial, 1),
            def("Bow", Domain::Martial, 3),
            def("Magic", Domain::Magic, 1),
            def("Pyromancy", Domain::Magic, 3),
            def("Arcane", Domain::Magic, 3),
            def("Hemomancy", Domain::Magic, 3),
            def("Bolt", Domain::Magic, 2),
            def("Burst", Domain::Magic, 2),
            def("Cone", Domain::Magic, 2),
            def("Beam", Domain::Magic, 2),
            def("Nova", Domain::Magic, 2),
            def("Chain", Domain::Magic, 2),
            def("Rune", Domain::Magic, 2),
            def("Summoning", Domain::Magic, 2),
            def("Beast lore", Domain::Lore, 1),
            def("Channeling", Domain::Magic, 1),
            def("Bloodletting", Domain::Magic, 1),
            def("Scroll lore", Domain::Magic, 1),
            def("Arcana", Domain::Lore, 1),
            def("Scavenging", Domain::Craft, 1),
        ])
        .unwrap()
    }

    #[test]
    fn levels_follow_the_curve() {
        assert_eq!(level(0), 0);
        assert_eq!(level(49), 0);
        assert_eq!(level(50), 1);
        assert_eq!(level(199), 1);
        assert_eq!(level(200), 2);
        assert_eq!(level(u32::MAX), MAX_LEVEL);
    }

    #[test]
    fn a_name_listed_twice_or_a_missing_core_skill_fails() {
        let mut defs = book().defs;
        defs.push(def("Dagger", Domain::Martial, 1));
        assert!(matches!(
            SkillBook::new(defs),
            Err(BlueprintError::DuplicateSkill(name)) if name == "Dagger"
        ));
        let defs = vec![def("Melee", Domain::Martial, 1)];
        assert!(matches!(
            SkillBook::new(defs),
            Err(BlueprintError::MissingSkill(name)) if name == "Unarmed"
        ));
    }

    fn id(state: &GameState, name: &str) -> SkillId {
        state.blueprints.skills().id(name).unwrap()
    }

    /// Sets `creature`'s levels by skill name.
    fn learn(state: &mut GameState, creature: Entity, levels: &[(&str, u32)]) {
        let mut skills = Skills::default();
        for &(name, level) in levels {
            skills.set(id(state, name), xp_for(level));
        }
        state.world.insert_one(creature, skills).unwrap();
    }

    fn goblin(state: &mut GameState, x: i32) -> Entity {
        thing("goblin").health(50).hostile().at(x, 1).make(state)
    }

    fn knife(state: &mut GameState, owner: Entity) -> Entity {
        let dagger = id(state, "Dagger");
        thing("knife")
            .weapon(1, 3, Stat::Agility)
            .with(Trains(vec![dagger]))
            .equipped_on(owner, 2)
            .make(state)
    }

    fn names(state: &GameState, skills: &[SkillId]) -> Vec<String> {
        let book = state.blueprints.skills();
        skills
            .iter()
            .map(|&id| book.get(id).unwrap().name.clone())
            .collect()
    }

    fn player_strike(state: &GameState) -> Strike {
        crate::combat::strike(&state.world, state.blueprints.skills(), state.player).unwrap()
    }

    #[test]
    fn a_strike_uses_melee_the_tool_and_its_kinds_of_damage() {
        let mut state = bare_state();
        let bare = player_strike(&state);
        let used = melee(&state.world, state.blueprints.skills(), state.player, &bare);
        assert_eq!(names(&state, &used), ["Melee", "Unarmed", "Blunt"]);

        let player = state.player;
        knife(&mut state, player);
        let armed = player_strike(&state);
        let used = melee(
            &state.world,
            state.blueprints.skills(),
            state.player,
            &armed,
        );
        assert_eq!(
            names(&state, &used),
            ["Melee", "Slash", "Dagger", "One-handed", "Empty offhand"]
        );

        // Something in the other hand: still one-handed, no longer empty.
        thing("buckler").equipped_on(player, 3).make(&mut state);
        let used = melee(&state.world, state.blueprints.skills(), player, &armed);
        assert_eq!(
            names(&state, &used),
            ["Melee", "Slash", "Dagger", "One-handed"]
        );
    }

    #[test]
    fn the_level_at_an_action_is_the_weighted_average() {
        let mut state = bare_state();
        let player = state.player;
        knife(&mut state, player);
        // Melee 8 (weight 1), Slash 0 (2), Dagger 4 (3), One-handed 6 (1),
        // Empty offhand 0 (1): (8 + 0 + 12 + 6 + 0) / 8 = 3, rounded down.
        learn(
            &mut state,
            player,
            &[("Melee", 8), ("Dagger", 4), ("One-handed", 6)],
        );
        assert_eq!(player_strike(&state).level, 3);
    }

    #[test]
    fn every_four_levels_add_a_point_of_damage_and_the_preview_agrees() {
        let mut state = bare_state();
        let player = state.player;
        sure_hits(&mut state, player);
        let target = goblin(&mut state, 2);
        let base = player_strike(&state).range();
        let player = state.player;
        learn(
            &mut state,
            player,
            &[("Melee", 9), ("Unarmed", 9), ("Blunt", 9)],
        );
        let skilled = player_strike(&state);
        assert_eq!(skilled.level, 9);
        assert_eq!(skilled.range(), (base.0 + 2, base.1 + 2));

        let preview = crate::combat::preview_damage(&state, player, target).unwrap();
        let before = state.world.get::<&Health>(target).unwrap().current;
        state.run(Command::Attack {
            attacker: player,
            target,
        });
        let after = state.world.get::<&Health>(target).unwrap().current;
        assert_eq!(before - after, preview);
    }

    #[test]
    fn hitting_an_enemy_trains_every_skill_used_less_each_time() {
        let mut state = bare_state();
        let player = state.player;
        sure_hits(&mut state, player);
        state.world.insert_one(player, Learner).unwrap();
        let target = goblin(&mut state, 2);
        let attack = Command::Attack {
            attacker: player,
            target,
        };
        state.run(attack);
        for skill in ["Melee", "Unarmed", "Blunt"] {
            assert_eq!(
                xp(&state.world, player, id(&state, skill)),
                BASE_XP,
                "{skill}"
            );
        }
        assert_eq!(xp(&state.world, player, id(&state, "Dagger")), 0);

        // The second goblin teaches 10 / 11 of the first.
        state.run(attack);
        let melee = id(&state, "Melee");
        assert_eq!(xp(&state.world, player, melee), BASE_XP + BASE_XP * 10 / 11);

        // Something new teaches in full again.
        let rat = thing("rat").health(50).hostile().at(1, 2).make(&mut state);
        state.run(Command::Attack {
            attacker: player,
            target: rat,
        });
        assert_eq!(
            xp(&state.world, player, melee),
            2 * BASE_XP + BASE_XP * 10 / 11
        );
    }

    #[test]
    fn only_a_learner_hitting_an_enemy_trains() {
        let mut state = bare_state();
        let player = state.player;
        let friend = thing("friend").health(50).at(2, 1).make(&mut state);
        state.world.insert_one(player, Learner).unwrap();
        state.run(Command::Attack {
            attacker: player,
            target: friend,
        });
        assert!(state.world.get::<&Skills>(player).is_err());

        let goblin = goblin(&mut state, 1);
        state.run(Command::Attack {
            attacker: goblin,
            target: player,
        });
        assert!(state.world.get::<&Skills>(goblin).is_err());
    }

    #[test]
    fn a_level_gained_is_logged() {
        let mut state = bare_state();
        let player = state.player;
        state.world.insert_one(player, Learner).unwrap();
        let mut skills = Skills::default();
        skills.set(id(&state, "Unarmed"), xp_for(1) - 1);
        state.world.insert_one(player, skills).unwrap();
        let target = goblin(&mut state, 2);
        state.run(Command::Attack {
            attacker: player,
            target,
        });
        assert!(
            state
                .log
                .recent(5)
                .any(|line| line == "Your unarmed skill rises to 1."),
        );
    }

    #[test]
    fn the_data_file_loads() {
        let path = crate::testkit::data().join("skills.ron");
        let book = SkillBook::load(&path).unwrap();
        assert!(book.core.is_some());
        assert!(book.id("Dagger").is_some());
    }
}
