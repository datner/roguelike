//! Read-only views of the player's state for frontends: plain data, no hecs
//! types, so frontends can draw panels without touching the world.

use hecs::{Entity, World};

use crate::blueprint::ResolvedBlueprint;
use crate::cell::Rgb;
use crate::combat::{Strike, strike};
use crate::entity::{
    Ammo, Appearance, Armor, Body, DamageType, DamageTypes, Equippable, Health, Launcher, Lesson,
    Light, Name, Note, PartKind, Projectile, Quiver, Scroll, Shape, Skills, SpawnSeq, Speed, Spell,
    Stat, Stats, Style, Throwable, Weapon,
};
use crate::equipment::equipped;
use crate::holding;
use crate::skill::{self, Domain, SkillId};
use crate::spell;
use crate::state::GameState;

/// A frontend's handle on an item: its `SpawnSeq`, which is unique, stable
/// across saves, and says nothing about hecs ids. Public, since a made-up
/// one is harmless: `take_turn` only resolves refs to items the player holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemRef(pub SpawnSeq);

impl ItemRef {
    pub(crate) fn of(world: &World, item: Entity) -> Option<Self> {
        world.get::<&SpawnSeq>(item).ok().map(|seq| Self(*seq))
    }

    /// The entity among `candidates` this refers to.
    pub(crate) fn find(self, world: &World, candidates: &[Entity]) -> Option<Entity> {
        candidates
            .iter()
            .copied()
            .find(|&item| world.get::<&SpawnSeq>(item).is_ok_and(|seq| *seq == self.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemView {
    pub item: ItemRef,
    /// Bare noun, e.g. "rusty knife".
    pub name: String,
    pub glyph: char,
    pub fg: Rgb,
    /// The kind of body part it can be equipped on, if any.
    pub fits: Option<PartKind>,
    /// Loose ammunition: it can be nocked or put in a quiver.
    pub ammo: bool,
    /// What it holds, to show beside the name: a quiver's arrows, a bow's
    /// nocked arrow.
    pub note: Option<String>,
    /// The skills using it trains, by name (ADR 0017), in skill-file order.
    pub trains: Vec<String>,
    /// It holds a lesson or a technique to study.
    pub lesson: bool,
    /// What it does, a line per part, for a panel's details.
    pub facts: Vec<String>,
}

/// The stat a roll adds, as the character sheet names it.
fn stat_name(stat: Stat) -> &'static str {
    match stat {
        Stat::Strength => "STR",
        Stat::Agility => "AGI",
        Stat::Intelligence => "INT",
        Stat::Willpower => "WIL",
    }
}

fn kinds(types: DamageTypes) -> String {
    types
        .iter()
        .map(DamageType::noun)
        .collect::<Vec<_>>()
        .join("+")
}

/// What `item` does, read from its parts: dice, reach, armor, light, notes.
fn facts(world: &World, item: Entity) -> Vec<String> {
    let mut facts = Vec::new();
    if let Ok(w) = world.get::<&Weapon>(item) {
        let (min, max, kinds, stat) = (w.min, w.max, kinds(w.types), stat_name(w.stat));
        facts.push(format!("melee {min}-{max} {kinds}, +{stat}"));
    }
    if let Ok(t) = world.get::<&Throwable>(item) {
        let (min, max, kinds, stat) = (t.min, t.max, kinds(t.types), stat_name(t.stat));
        let range = t.range;
        facts.push(format!(
            "thrown {min}-{max} {kinds} up to {range} tiles, +{stat}"
        ));
    }
    if let Ok(l) = world.get::<&Launcher>(item) {
        let (range, steady, stat) = (l.range, l.steady, stat_name(l.stat));
        facts.push(format!(
            "shoots arrows up to {range} tiles, steady to {steady}, +{stat}"
        ));
    }
    if let Ok(p) = world.get::<&Projectile>(item) {
        let (min, max, kinds) = (p.min, p.max, kinds(p.types));
        let breaks = p.break_bp / 100;
        facts.push(format!("hits for {min}-{max} {kinds}, {breaks}% to break"));
    }
    if let Ok(q) = world.get::<&Quiver>(item) {
        facts.push(format!("holds {} arrows", q.capacity));
    }
    if let Ok(armor) = world.get::<&Armor>(item) {
        let values: Vec<String> = DamageType::ALL
            .iter()
            .filter_map(|&kind| {
                let value = armor.of(kind);
                (value != 0).then(|| format!("{} {value}", kind.noun()))
            })
            .collect();
        if !values.is_empty() {
            facts.push(format!("armor: {}", values.join(", ")));
        }
    }
    if let Ok(light) = world.get::<&Light>(item) {
        facts.push(format!("lights {} tiles around", light.radius));
    }
    if let Ok(note) = world.get::<&Note>(item) {
        facts.push(note.0.clone());
    }
    facts
}

/// A quiver's load or a launcher's nocked arrow, for display.
fn note(world: &World, item: Entity) -> Option<String> {
    if let Ok(quiver) = world.get::<&Quiver>(item) {
        return Some(format!(
            "{}/{}",
            holding::carried(world, item).len(),
            quiver.capacity
        ));
    }
    if world.satisfies::<&Launcher>(item) {
        let arrow = holding::carried(world, item).into_iter().next()?;
        let name = world.get::<&Name>(arrow).ok()?;
        return Some(format!("nocked: {}", name.0));
    }
    None
}

impl ItemView {
    fn of(state: &GameState, item: Entity) -> Option<Self> {
        let world = &state.world;
        let book = state.blueprints.skills();
        let name = world
            .get::<&Name>(item)
            .map_or_else(|_| "thing".to_string(), |n| n.0.clone());
        let (glyph, fg) = world
            .get::<&Appearance>(item)
            .map_or(('?', Rgb(200, 200, 200)), |look| (look.glyph, look.fg));
        Some(Self {
            item: ItemRef::of(world, item)?,
            name,
            glyph,
            fg,
            fits: world.get::<&Equippable>(item).ok().map(|e| e.part),
            ammo: world.satisfies::<&Ammo>(item),
            note: note(world, item),
            lesson: world.satisfies::<&Lesson>(item)
                || world.satisfies::<&crate::entity::Teaches>(item),
            trains: skill::of_item(world, book, item)
                .into_iter()
                .filter_map(|id| book.get(id).map(|def| def.name.clone()))
                .collect(),
            facts: facts(world, item),
        })
    }
}

/// What the player carries, in the order it was picked up.
pub fn inventory_view(state: &GameState) -> Vec<ItemView> {
    carried(state)
        .into_iter()
        .filter_map(|item| ItemView::of(state, item))
        .collect()
}

/// One of the player's body parts and what is equipped on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartView {
    /// Pass back as `PlayerAction::Equip { part: Some(index), .. }`.
    pub index: u32,
    pub name: String,
    pub kind: PartKind,
    pub item: Option<ItemView>,
}

/// The player's body parts in anatomy order, equipped or empty.
pub fn equipment_view(state: &GameState) -> Vec<PartView> {
    let world = &state.world;
    let Ok(body) = world.get::<&Body>(state.player) else {
        return Vec::new();
    };
    let worn = equipped(world, state.player);
    (0..)
        .zip(&body.0)
        .map(|(index, part)| PartView {
            index,
            name: part.name.clone(),
            kind: part.kind,
            item: worn
                .iter()
                .find(|&&(_, at)| at == index)
                .and_then(|&(item, _)| ItemView::of(state, item)),
        })
        .collect()
}

/// The player's numbers, for a character panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterSheet {
    pub health: i32,
    pub max_health: i32,
    /// Current and maximum mana, read from the timestamps; `None` without a pool.
    pub mana: Option<(u32, u32)>,
    /// Ticks per mana point regained, for the player's stats; 0 never.
    pub mana_regen_ticks: Option<u64>,
    pub stats: Option<Stats>,
    /// Resistance per kind of damage: the player's own plus everything equipped.
    pub armor: Armor,
    /// Melee damage before armor, stat bonus included; `None` if unable.
    pub damage: Option<(i32, i32)>,
    /// The kinds of that damage.
    pub damage_types: DamageTypes,
    /// What melee hits are made with, if not bare-handed.
    pub wielding: Option<String>,
    pub speed: u32,
    /// The origin's name, as the menu showed it; `None` without one.
    pub origin: Option<String>,
}

/// How the player died (docs/plans/legibility.md): the chain, not just the
/// last blow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathView {
    /// "the goblin's blow", "the fall into the chasm", "bleeding".
    pub killer: String,
    /// The damage that killed.
    pub amount: i32,
    /// Enemies beside you beyond the first, as you fell.
    pub outnumbered: u32,
    /// The rest budget left on the floor; `None` at camp.
    pub rest_left: Option<u32>,
    /// "The caves, floor 3".
    pub place: String,
}

/// How the player died, once they have.
pub fn death(state: &GameState) -> Option<&DeathView> {
    state.death.as_ref()
}

/// The player's chance to hit with a blow, and why (docs/plans/legibility.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitChance {
    /// Out of 100.
    pub percent: u32,
    /// What made it, in percentage points; they add up to `percent`.
    pub reasons: Vec<(String, i32)>,
}

/// The player's chance to land a blow on the creature at `at`, from the same
/// odds the blow rolls; `None` if nothing there can be struck.
pub fn hit_chance(state: &GameState, at: (i32, i32)) -> Option<HitChance> {
    let player = state.player;
    let target = state
        .world
        .query::<(Entity, &crate::entity::Position, &crate::entity::Health)>()
        .without::<&crate::entity::Dead>()
        .iter()
        .find(|&(other, pos, _)| other != player && (pos.x, pos.y) == at)
        .map(|(other, _, _)| other)?;
    let strike = crate::combat::strike(&state.world, state.blueprints.skills(), player)?;
    let (odds, reasons) = crate::combat::odds_explained(state, &strike, player, target);
    Some(HitChance {
        percent: odds.hit_bp / 100,
        reasons: reasons
            .into_iter()
            .map(|reason| (reason.what, reason.bp / 100))
            .collect(),
    })
}

/// How many enemies stand beside the player beyond the first, each making
/// the player easier to hit (`combat::outnumbered`).
pub fn outnumbered(state: &GameState) -> u32 {
    crate::combat::outnumbered(state, state.player)
}

pub fn character_sheet(state: &GameState) -> CharacterSheet {
    let world = &state.world;
    let player = state.player;
    let (health, max_health) = world
        .get::<&Health>(player)
        .map_or((0, 0), |h| (h.current, h.max));
    let armor = total_armor(world, player);
    let strike = strike(world, state.blueprints.skills(), player);
    CharacterSheet {
        health,
        max_health,
        mana: spell::pool(world, player)
            .map(|(mana, rates)| (mana.current(rates, state.now), rates.max)),
        mana_regen_ticks: spell::pool(world, player).map(|(_, rates)| rates.regen_ticks),
        stats: world.get::<&Stats>(player).ok().map(|stats| *stats),
        armor,
        damage: strike.map(Strike::range),
        damage_types: strike.map_or_else(DamageTypes::default, |strike| strike.types),
        wielding: strike
            .and_then(|strike| strike.weapon)
            .and_then(|weapon| world.get::<&Name>(weapon).ok().map(|name| name.0.clone())),
        speed: world.get::<&Speed>(player).map_or(0, |speed| speed.0),
        origin: world
            .get::<&crate::entity::Origin>(player)
            .ok()
            .and_then(|origin| state.blueprints.origin(&origin.0))
            .map(|origin| origin.name.clone()),
    }
}

pub(crate) fn carried(state: &GameState) -> Vec<Entity> {
    // Known styles are carried too (ADR 0019), but they are not things.
    holding::carried(&state.world, state.player)
        .into_iter()
        .filter(|&thing| !state.world.satisfies::<&Style>(thing))
        .collect()
}

/// Everything the player has in hand or on: carried, then equipped.
pub(crate) fn held(state: &GameState) -> Vec<Entity> {
    let mut held = carried(state);
    held.extend(
        equipped(&state.world, state.player)
            .into_iter()
            .map(|(item, _)| item),
    );
    held
}

/// The spells the player knows, in the order they were learned.
pub(crate) fn known_spells(state: &GameState) -> Vec<Entity> {
    holding::attached(&state.world, state.player)
        .into_iter()
        .filter(|&spell| state.world.satisfies::<&Spell>(spell))
        .collect()
}

/// What the player can cast: the spells they know, then the scrolls they
/// carry.
pub(crate) fn castable(state: &GameState) -> Vec<Entity> {
    let mut castable = known_spells(state);
    castable.extend(
        carried(state)
            .into_iter()
            .filter(|&item| state.world.satisfies::<(&Spell, &Scroll)>(item)),
    );
    castable
}

/// One price a cast takes, as the discipline that takes it describes it:
/// "2 mana", "6 turn recharge".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostLine {
    pub resource: String,
    pub amount: u64,
}

/// What casting a spell now would take, or why it can't be cast. Built from
/// the disciplines' `CheckCast` answers, so a new way to pay needs no
/// frontend change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastPreview {
    pub costs: Vec<CostLine>,
    pub blocked: Option<String>,
    /// Chance, out of 10 000, that the cast fizzles once paid for.
    pub fizzle_bp: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellView {
    pub spell: ItemRef,
    /// Bare noun, e.g. "firebolt".
    pub name: String,
    pub range: i32,
    /// The ground it covers; `None` for a bolt.
    pub shape: Option<Shape>,
    /// Damage range before the target's defences; `None` if it does none.
    pub damage: Option<(i32, i32)>,
    /// How many creatures it calls, and what they are.
    pub summons: Option<(u32, String)>,
    pub preview: CastPreview,
}

/// What the player can cast (`castable`), in that order.
pub fn spells(state: &GameState) -> Vec<SpellView> {
    let world = &state.world;
    castable(state)
        .into_iter()
        .filter_map(|spell| {
            let (ready, blocked) = spell::inspect_cast(state, state.player, spell).ok()?;
            Some(SpellView {
                spell: ItemRef::of(world, spell)?,
                name: world
                    .get::<&Name>(spell)
                    .map_or_else(|_| "spell".to_string(), |name| name.0.clone()),
                range: ready.range,
                shape: ready.area.map(|area| area.shape),
                damage: ready.strike.map(Strike::range),
                summons: ready.summon.map(|summon| {
                    let noun = state
                        .blueprints
                        .by_id(summon.blueprint)
                        .and_then(ResolvedBlueprint::noun)
                        .unwrap_or("creature");
                    (summon.count, noun.to_string())
                }),
                preview: CastPreview {
                    costs: ready.costs,
                    blocked,
                    fizzle_bp: ready.fizzle_bp,
                },
            })
        })
        .collect()
}

/// One skill the player has, for a skills panel (ADR 0017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillView {
    pub name: String,
    pub domain: Domain,
    pub level: u32,
    pub xp: u32,
    /// Total XP the next level needs; `None` at the highest level.
    pub next: Option<u32>,
}

/// The skills the player has any XP in, by domain, then in skill-file
/// order. Weightless tags are left out.
pub fn skills(state: &GameState) -> Vec<SkillView> {
    let book = state.blueprints.skills();
    let Ok(learned) = state.world.get::<&Skills>(state.player) else {
        return Vec::new();
    };
    let mut views: Vec<(Domain, SkillId, SkillView)> = learned
        .0
        .iter()
        .filter(|&&(_, xp)| xp > 0)
        .filter_map(|&(id, xp)| {
            // A weightless skill is a tag (the short bow), not something to
            // grow: it never counts toward a level.
            let def = book.get(id).filter(|def| def.weight > 0)?;
            let level = skill::level(xp);
            let view = SkillView {
                name: def.name.clone(),
                domain: def.domain,
                level,
                xp,
                next: (level < skill::MAX_LEVEL).then(|| skill::xp_for(level + 1)),
            };
            Some((def.domain, id, view))
        })
        .collect();
    views.sort_by_key(|&(domain, id, _)| (domain, id));
    views.into_iter().map(|(_, _, view)| view).collect()
}

/// What the player could throw right now.
pub fn throwables(state: &GameState) -> Vec<ItemView> {
    held(state)
        .into_iter()
        .filter(|&item| state.world.satisfies::<&Throwable>(item))
        .filter_map(|item| ItemView::of(state, item))
        .collect()
}

/// Armor of everything `entity` is and wears: its own plus its equipment.
pub(crate) fn total_armor(world: &World, entity: Entity) -> Armor {
    let armor_of = |thing| {
        world
            .get::<&Armor>(thing)
            .map_or(Armor::default(), |armor| *armor)
    };
    equipped(world, entity)
        .into_iter()
        .fold(armor_of(entity), |sum, (item, _)| sum.plus(armor_of(item)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Stat, Trains};
    use crate::input::PlayerAction;

    fn data_state() -> GameState {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        crate::state::tests::bare_state_with(crate::Blueprints::load(&data).unwrap())
    }

    #[test]
    fn the_hit_chance_on_a_creature_comes_with_its_reasons() {
        let mut state = crate::state::tests::bare_state();
        crate::testkit::thing("goblin")
            .health(5)
            .hostile()
            .at(2, 1)
            .make(&mut state);
        let chance = hit_chance(&state, (2, 1)).unwrap();
        assert_eq!(chance.percent, 75);
        assert_eq!(chance.reasons.iter().map(|(_, p)| p).sum::<i32>(), 75);
        assert_eq!(hit_chance(&state, (5, 5)), None, "nothing there");
    }

    #[test]
    fn the_short_bow_tag_trains_but_counts_toward_no_level_and_is_not_listed() {
        let mut state = data_state();
        let book = state.blueprints.skills().clone();
        let id = |name| book.id(name).unwrap();
        let (bow, archery, tag) = (id("Bow"), id("Archery"), id("Short bow"));
        let player = state.player;
        let mut learned = Skills::default();
        learned.set(bow, skill::xp_for(6));
        learned.set(archery, skill::xp_for(2));
        state.world.insert_one(player, learned).unwrap();
        let level = |state: &GameState, skills: &[SkillId]| {
            skill::effective(&state.world, &book, player, skills)
        };
        assert_eq!(
            level(&state, &[bow, archery, tag]),
            level(&state, &[bow, archery])
        );

        // The player starts with a short bow in hand: bashing with it trains
        // the tag.
        let goblin = crate::testkit::thing("goblin")
            .hostile()
            .health(1000)
            .at(2, 1)
            .make(&mut state);
        crate::testkit::sure_hits(&mut state, player);
        state.run(crate::command::Command::TryMove(
            player,
            crate::input::Dir::E,
        ));
        assert!(state.world.contains(goblin));
        assert!(skill::xp(&state.world, player, tag) > 0);
        assert!(skills(&state).iter().all(|view| view.name != "Short bow"));
        assert!(skills(&state).iter().any(|view| view.name == "Bow"));
    }

    #[test]
    fn skills_list_those_with_xp_by_domain_and_items_list_what_they_train() {
        let mut state = bare_state();
        let book = state.blueprints.skills();
        let id = |name| book.id(name).unwrap();
        let (dodge, dagger, melee) = (id("Dodge"), id("Dagger"), id("Melee"));
        let player = state.player;
        let learned = Skills(vec![(melee, 0), (dagger, skill::xp_for(2) + 5), (dodge, 1)]);
        state.world.insert_one(player, learned).unwrap();
        let shown: Vec<(String, Domain, u32, u32, Option<u32>)> = skills(&state)
            .into_iter()
            .map(|s| (s.name, s.domain, s.level, s.xp, s.next))
            .collect();
        assert_eq!(
            shown,
            [
                ("Dagger".into(), Domain::Martial, 2, 205, Some(450)),
                ("Dodge".into(), Domain::Defence, 0, 1, Some(50)),
            ]
        );

        thing("knife")
            .item()
            .weapon(1, 3, Stat::Agility)
            .with(Trains(vec![dagger]))
            .carried_by(player)
            .make(&mut state);
        assert_eq!(inventory_view(&state)[0].trains, ["Slash", "Dagger"]);
    }
    use crate::state::tests::bare_state;
    use crate::testkit::{last_log, thing};
    use crate::time::take_turn;

    fn knife_at(state: &mut GameState, x: i32, y: i32) -> Entity {
        thing("knife")
            .look('/', Rgb(1, 2, 3))
            .item()
            .at(x, y)
            .make(state)
    }

    #[test]
    fn the_inventory_view_lists_what_was_picked_up() {
        let mut state = bare_state();
        knife_at(&mut state, 1, 1);
        assert!(inventory_view(&state).is_empty());

        take_turn(&mut state, PlayerAction::PickUp);

        let view = inventory_view(&state);
        assert_eq!(view.len(), 1);
        assert_eq!((view[0].name.as_str(), view[0].glyph), ("knife", '/'));
        assert_eq!(view[0].fg, Rgb(1, 2, 3));
    }

    #[test]
    fn picking_up_and_dropping_take_a_turn_each() {
        let mut state = bare_state();
        knife_at(&mut state, 1, 1);
        let start = state.now;
        take_turn(&mut state, PlayerAction::PickUp);
        let picked = state.now;
        assert!(picked > start);

        let item = inventory_view(&state)[0].item;
        take_turn(&mut state, PlayerAction::Drop(item));
        assert!(state.now > picked);
        assert!(inventory_view(&state).is_empty());
    }

    #[test]
    fn picking_up_nothing_is_refused_without_spending_time() {
        let mut state = bare_state();
        let start = state.now;
        take_turn(&mut state, PlayerAction::PickUp);
        assert_eq!(state.now, start);
        assert_eq!(last_log(&state), "There is nothing here to pick up.");
    }

    #[test]
    fn the_character_sheet_combines_stats_and_equipment() {
        use crate::entity::{PartKind, Stat};

        let mut state = bare_state();
        let player = state.player;
        state
            .world
            .insert_one(
                player,
                Stats {
                    strength: 18,
                    agility: 20,
                    ..Stats::AVERAGE
                },
            )
            .unwrap();
        let sheet = character_sheet(&state);
        assert_eq!(
            (sheet.health, sheet.max_health, sheet.armor, sheet.speed),
            (20, 20, Armor::default(), 100)
        );
        // Test player: natural 2-4, +1 Strength.
        assert_eq!((sheet.damage, sheet.wielding), (Some((3, 5)), None));

        let blade = thing("knife")
            .item()
            .at(1, 1)
            .fits(PartKind::Hand)
            .weapon(1, 3, Stat::Agility)
            .make(&mut state);
        let mail = thing("mail")
            .item()
            .at(1, 1)
            .fits(PartKind::Body)
            .armor(2)
            .make(&mut state);
        take_turn(&mut state, PlayerAction::PickUp);
        for item in [blade, mail] {
            let item = ItemRef::of(&state.world, item).unwrap();
            take_turn(&mut state, PlayerAction::Equip { item, part: None });
        }

        let sheet = character_sheet(&state);
        assert_eq!(sheet.armor, Armor::uniform(2));
        // Knife 1-3, +2 Agility.
        assert_eq!(sheet.damage, Some((3, 5)));
        assert_eq!(sheet.wielding.as_deref(), Some("knife"));
    }

    #[test]
    fn dropping_an_item_ref_you_do_not_carry_is_refused() {
        let mut state = bare_state();
        let knife = knife_at(&mut state, 3, 3);
        let item = ItemRef::of(&state.world, knife).unwrap();
        let start = state.now;
        take_turn(&mut state, PlayerAction::Drop(item));
        assert_eq!(state.now, start);
        assert_eq!(last_log(&state), "You do not carry that.");
    }
}
