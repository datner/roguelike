//! `Command` and `apply_commands`: the only place the world is mutated during play (ADR 0002).

use std::ops::ControlFlow;

use hecs::Entity;

use crate::ammo;
use crate::blueprint::BlueprintId;
use crate::combat::{AfterHit, BeforeTakeDamage, Damaged, roll_melee};
use crate::cost;
use crate::effect::{self, DurationChange};
use crate::entity::{
    Armor, BlocksMovement, DamageTypes, Dead, Energy, Found, Health, Item, Position, SpawnSeq,
};
use crate::equipment;
use crate::event::{Ctx, settle};
use crate::fov;
use crate::grammar::{Subject, capitalize};
use crate::holding;
use crate::input::Dir;
use crate::map::Tile;
use crate::ranged;
use crate::spell;
use crate::state::GameState;
use crate::time;

/// Upper bound on commands applied per `apply_commands` call, so a handler
/// feedback loop degrades into an error message instead of a hang. Each
/// scheduler step (one action, one timer firing, one clock jump) drains the
/// queue in its own call, so the budget is per step: a crowd of actors
/// between two player turns never shares it.
pub const MAX_COMMANDS_PER_STEP: usize = 1000;

/// Where a hit came from. Only `Melee` hits trigger on-hit effects, so an
/// effect's own damage can never re-apply effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DamageCause {
    Melee {
        attacker: Entity,
        types: DamageTypes,
    },
    /// Thrown or fired: armored against like a blow, but not an `AfterHit`
    /// source, so a missile never triggers its thrower's on-hit parts.
    Ranged {
        attacker: Entity,
        types: DamageTypes,
    },
    /// A spell: untyped, so armor passes it, and not an `AfterHit` source.
    Spell {
        caster: Entity,
    },
    Effect {
        effect: Entity,
    },
}

impl DamageCause {
    /// What armor can resist. Effect damage is of no kind, so it passes.
    pub(crate) fn types(self) -> DamageTypes {
        match self {
            Self::Melee { types, .. } | Self::Ranged { types, .. } => types,
            Self::Effect { .. } | Self::Spell { .. } => DamageTypes::default(),
        }
    }
}

/// Actions (`TryMove`, `Wait`, `PickUp`, `Drop`, `Equip`, `Unequip`) cost
/// their actor energy;
/// see `cost::of`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    TryMove(Entity, Dir),
    /// Do nothing for a standard action.
    Wait(Entity),
    /// Take every floor item on the actor's tile into their `Inventory`.
    PickUp(Entity),
    /// Put `item` from the actor's `Inventory` down on their tile.
    Drop {
        actor: Entity,
        item: Entity,
    },
    /// Move `item` from the actor's `Inventory` onto a body part: `part`,
    /// or the first that fits. Whatever was there goes back to the inventory.
    Equip {
        actor: Entity,
        item: Entity,
        part: Option<u32>,
    },
    /// Move an equipped `item` back into the actor's `Inventory`.
    Unequip {
        actor: Entity,
        item: Entity,
    },
    /// Hurl `item`, carried or equipped, at the tile `at`. It flies to the
    /// first creature or wall on the line (ADR 0013) and comes to rest there.
    Throw {
        actor: Entity,
        item: Entity,
        at: (i32, i32),
    },
    /// Shoot the launcher `actor` wields at the tile `at`, with the nocked
    /// arrow or else the next one in an equipped quiver.
    Fire {
        actor: Entity,
        at: (i32, i32),
    },
    /// Put `arrow` from the pack on the wielded launcher (a turn, even with a
    /// quiver: a special arrow overrides the quiver, ADR 0013).
    Nock {
        actor: Entity,
        arrow: Entity,
    },
    /// Move loose ammunition from the pack into an equipped quiver.
    Quiver {
        actor: Entity,
        arrow: Entity,
    },
    /// Cast `spell`, which `actor` knows, at the tile `at` (ADR 0014).
    Cast {
        actor: Entity,
        spell: Entity,
        at: (i32, i32),
    },
    /// Start `spell`'s recharge: it cannot be cast for `duration` ticks.
    Recharge {
        spell: Entity,
        duration: u64,
    },
    Attack {
        attacker: Entity,
        target: Entity,
    },
    DealDamage {
        target: Entity,
        amount: i32,
        cause: DamageCause,
    },
    Die {
        entity: Entity,
    },
    /// Spawn the effect `blueprint` (which must carry a `Duration`), start it
    /// now, and attach it to `target`.
    ApplyEffect {
        target: Entity,
        blueprint: BlueprintId,
    },
    /// The only way an effect's duration changes (ADR 0006).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "nothing changes durations in play yet; the mechanism is tested"
        )
    )]
    ModifyDuration {
        effect: Entity,
        change: DurationChange,
    },
    ExpireEffect(Entity),
    BleedTick(Entity),
    /// The player arrived on a tile: the floor items there become `Found`.
    FindItems(Entity),
    /// Scheduler only: move the clock forward and grant actors energy.
    AdvanceClock {
        ticks: u64,
    },
    /// Scheduler only: recompute the player's field of view and remember
    /// what it shows. Queued once per player turn (ADR 0007).
    UpdateFov,
}

impl Command {
    /// The entities a command acts for or on. If any of them is `Dead`,
    /// `apply_commands` skips the command.
    fn participants(&self) -> [Option<Entity>; 2] {
        match *self {
            Command::TryMove(actor, _)
            | Command::Wait(actor)
            | Command::Fire { actor, .. }
            | Command::PickUp(actor)
            | Command::FindItems(actor) => [Some(actor), None],
            Command::Drop { actor, item }
            | Command::Equip { actor, item, .. }
            | Command::Unequip { actor, item }
            | Command::Throw { actor, item, .. }
            | Command::Nock { actor, arrow: item }
            | Command::Quiver { actor, arrow: item } => [Some(actor), Some(item)],
            Command::Cast { actor, spell, .. } => [Some(actor), Some(spell)],
            Command::Recharge { spell, .. } => [Some(spell), None],
            Command::Attack { attacker, target } => [Some(attacker), Some(target)],
            Command::DealDamage { target, .. } | Command::ApplyEffect { target, .. } => {
                [Some(target), None]
            }
            Command::Die { entity } => [Some(entity), None],
            Command::ModifyDuration { effect, .. }
            | Command::ExpireEffect(effect)
            | Command::BleedTick(effect) => [Some(effect), None],
            Command::AdvanceClock { .. } | Command::UpdateFov => [None, None],
        }
    }
}

impl GameState {
    /// Queues `command` and applies it with everything it triggers. A
    /// method, so arguments may read `self` (`state.run(Wait(state.player))`).
    pub(crate) fn run(&mut self, command: Command) {
        self.commands.push_back(command);
        apply_commands(self);
    }
}

/// Drains the command queue. This is the only code that mutates the world.
pub(crate) fn apply_commands(state: &mut GameState) {
    let mut applied = 0;
    while let Some(command) = state.commands.pop_front() {
        if applied == MAX_COMMANDS_PER_STEP {
            let dropped = state.commands.len() + 1;
            state.commands.clear();
            state.log.push(format!(
                "Error: command limit ({MAX_COMMANDS_PER_STEP}) reached; dropped {dropped} queued commands."
            ));
            return;
        }
        applied += 1;
        // The one place `Dead` is checked: nothing acts for or on the dead,
        // so a dying actor's queued follow-ups (damage, attacks) fizzle.
        if command
            .participants()
            .into_iter()
            .flatten()
            .any(|entity| state.world.satisfies::<&Dead>(entity))
        {
            continue;
        }
        if let Some((actor, cost)) = cost::of(&command) {
            spend_energy(state, actor, cost);
        }
        match command {
            Command::TryMove(entity, dir) => try_move(state, entity, dir),
            Command::Wait(_) => {}
            Command::PickUp(actor) => pick_up(state, actor),
            Command::Drop { actor, item } => drop_item(state, actor, item),
            Command::Equip { actor, item, part } => equipment::equip(state, actor, item, part),
            Command::Unequip { actor, item } => equipment::unequip(state, actor, item),
            Command::Throw { actor, item, at } => ranged::throw(state, actor, item, at),
            Command::Cast { actor, spell, at } => spell::cast(state, actor, spell, at),
            Command::Recharge { spell, duration } => spell::recharge(state, spell, duration),
            Command::Fire { actor, at } => ranged::fire(state, actor, at),
            Command::Nock { actor, arrow } => ammo::nock(state, actor, arrow),
            Command::Quiver { actor, arrow } => ammo::quiver(state, actor, arrow),
            Command::Attack { attacker, target } => attack(state, attacker, target),
            Command::DealDamage {
                target,
                amount,
                cause,
            } => deal_damage(state, target, amount, cause),
            Command::Die { entity } => die(state, entity),
            Command::ApplyEffect { target, blueprint } => {
                effect::apply_effect(state, target, blueprint);
            }
            Command::ModifyDuration { effect, change } => {
                effect::modify_duration(state, effect, change);
            }
            Command::ExpireEffect(effect) => effect::expire_effect(state, effect),
            Command::BleedTick(effect) => effect::bleed_tick(state, effect),
            Command::AdvanceClock { ticks } => time::advance_clock(state, ticks),
            Command::UpdateFov => fov::update(state),
            Command::FindItems(finder) => find_items(state, finder),
        }
    }
}

/// Fires `ev` at `target` and queues whatever commands the handlers emitted.
pub(crate) fn dispatch<E: 'static>(
    state: &mut GameState,
    target: Entity,
    ev: E,
) -> ControlFlow<E, E> {
    let mut out = Vec::new();
    let mut ctx = Ctx {
        world: &state.world,
        map: &state.map,
        // Every handler today is combat; give events their own stream (and a
        // new `rng::stream` id) when another subsystem starts firing them.
        rng: &mut state.rngs.combat,
        out: &mut out,
    };
    let flow = state.events.fire(&mut ctx, target, ev);
    state.commands.extend(out);
    flow
}

/// Actors without `Energy` (not scheduled, e.g. in tests) act for free.
fn spend_energy(state: &mut GameState, actor: Entity, cost: i64) {
    if let Ok(mut energy) = state.world.get::<&mut Energy>(actor) {
        energy.0 = energy.0.saturating_sub(cost);
    }
}

/// Screens the player's input before it becomes an action. A move into a
/// wall (any wall, seen or not, for now), a pick-up with nothing to take,
/// or an equip or unequip that can't happen is refused with a message: nothing is queued, no energy is spent and no
/// time passes. Returns whether the input was refused.
pub(crate) fn reject_input(state: &mut GameState, command: &Command) -> bool {
    match *command {
        Command::TryMove(entity, dir) => {
            if !destination(state, entity, dir).is_some_and(|to| is_wall(state, to)) {
                return false;
            }
            log_wall_bump(state, entity);
        }
        Command::PickUp(entity) => {
            if !floor_items_under(state, entity).is_empty() {
                return false;
            }
            state.log.push("There is nothing here to pick up.");
        }
        Command::Equip { actor, item, part } => {
            let Err(message) = equipment::plan_equip(state, actor, item, part) else {
                return false;
            };
            state.log.push(message);
        }
        Command::Unequip { actor, item } => {
            let Err(message) = equipment::check_unequip(state, actor, item) else {
                return false;
            };
            state.log.push(message);
        }
        Command::Throw { actor, item, at } => {
            let Err(message) = ranged::check_throw(state, actor, item, at) else {
                return false;
            };
            state.log.push(message);
        }
        Command::Cast { actor, spell, at } => {
            let Err(message) = spell::check_cast(state, actor, spell, at) else {
                return false;
            };
            state.log.push(message);
        }
        Command::Fire { actor, at } => {
            let Err(message) = ranged::check_fire(state, actor, at) else {
                return false;
            };
            state.log.push(message);
        }
        Command::Nock { actor, arrow } => {
            let Err(message) = ammo::check_nock(state, actor, arrow) else {
                return false;
            };
            state.log.push(message);
        }
        Command::Quiver { actor, arrow } => {
            let Err(message) = ammo::check_quiver(state, actor, arrow) else {
                return false;
            };
            state.log.push(message);
        }
        _ => return false,
    }
    true
}

fn destination(state: &GameState, entity: Entity, dir: Dir) -> Option<(i32, i32)> {
    let (dx, dy) = dir.delta();
    let current = state.world.get::<&Position>(entity).ok()?;
    Some((current.x + dx, current.y + dy))
}

/// Off-map counts as wall.
fn is_wall(state: &GameState, (x, y): (i32, i32)) -> bool {
    !Tile::is_open(state.map.get(x, y))
}

fn log_wall_bump(state: &mut GameState, entity: Entity) {
    let mover = Subject::of(&state.world, entity);
    state.log.push(mover.sentence("bump", "into a wall"));
}

fn try_move(state: &mut GameState, entity: Entity, dir: Dir) {
    let Some(target) = destination(state, entity, dir) else {
        return;
    };

    // AI moves still pay for a bump; the player's are screened out by
    // `reject_input` before they get here.
    if is_wall(state, target) {
        log_wall_bump(state, entity);
        return;
    }

    let victim = state
        .world
        .query::<(Entity, &Position, &Health)>()
        .iter()
        .find(|&(other, pos, _)| other != entity && (pos.x, pos.y) == target)
        .map(|(other, _, _)| other);
    if let Some(victim) = victim {
        state.commands.push_back(Command::Attack {
            attacker: entity,
            target: victim,
        });
        return;
    }

    let blocked = state
        .world
        .query::<(&Position, &BlocksMovement)>()
        .iter()
        .any(|(pos, _)| (pos.x, pos.y) == target);
    if blocked {
        let mover = Subject::of(&state.world, entity);
        state
            .log
            .push(format!("Something blocks {} way.", mover.possessive()));
        return;
    }

    if let Ok(mut pos) = state.world.get::<&mut Position>(entity) {
        pos.x = target.0;
        pos.y = target.1;
    }
    if entity == state.player {
        state.commands.push_back(Command::FindItems(entity));
    }
}

/// Announces each floor item under `finder` the first time they stand on it.
fn find_items(state: &mut GameState, finder: Entity) {
    let new: Vec<Entity> = floor_items_under(state, finder)
        .into_iter()
        .filter(|&item| !state.world.satisfies::<&Found>(item))
        .collect();
    for item in new {
        let what = Subject::of(&state.world, item);
        let message = Subject::of(&state.world, finder).sentence("find", what.noun());
        state.log.push(message);
        let _ = state.world.insert_one(item, Found);
    }
}

/// The floor items on `entity`'s tile, in spawn order.
fn floor_items_under(state: &GameState, entity: Entity) -> Vec<Entity> {
    let Some(at) = state.world.get::<&Position>(entity).ok().map(|pos| *pos) else {
        return Vec::new();
    };
    let mut items: Vec<(Option<SpawnSeq>, Entity)> = state
        .world
        .query::<(Entity, &Position, &Item, Option<&SpawnSeq>)>()
        .iter()
        .filter(|&(item, pos, _, _)| *pos == at && item != entity)
        .map(|(item, _, _, seq)| (seq.copied(), item))
        .collect();
    items.sort();
    items.into_iter().map(|(_, item)| item).collect()
}

fn pick_up(state: &mut GameState, actor: Entity) {
    for item in floor_items_under(state, actor) {
        let _ = state.world.remove_one::<Position>(item);
        if ammo::stow_found(state, actor, item) {
            continue;
        }
        let what = Subject::of(&state.world, item);
        let message =
            Subject::of(&state.world, actor).sentence("pick", &format!("up {}", what.noun()));
        state.log.push(message);
        holding::stow(&mut state.world, actor, item);
    }
}

fn drop_item(state: &mut GameState, actor: Entity, item: Entity) {
    let carried = holding::carried(&state.world, actor).contains(&item);
    let at = state.world.get::<&Position>(actor).ok().map(|pos| *pos);
    let (true, Some(at)) = (carried, at) else {
        let message = Subject::of(&state.world, actor).sentence("do", "not carry that");
        state.log.push(message);
        return;
    };
    holding::unstow(&mut state.world, actor, item);
    // It lands where its dropper stands, so it counts as found.
    let _ = state.world.insert(item, (at, Found));
    let what = Subject::of(&state.world, item);
    let message = Subject::of(&state.world, actor).sentence("drop", what.noun());
    state.log.push(message);
}

fn attack(state: &mut GameState, attacker: Entity, target: Entity) {
    if let Some(hit) = roll_melee(&state.world, &mut state.rngs.combat, attacker) {
        state.commands.push_back(Command::DealDamage {
            target,
            amount: hit.amount,
            cause: DamageCause::Melee {
                attacker,
                types: hit.types,
            },
        });
    }
}

fn deal_damage(state: &mut GameState, target: Entity, amount: i32, cause: DamageCause) {
    if !state.world.satisfies::<&Health>(target) {
        return;
    }

    let dealt = settle(dispatch(
        state,
        target,
        BeforeTakeDamage {
            target,
            amount,
            armor: Armor::default(),
            cause,
        },
    ))
    .dealt();
    let victim = Subject::of(&state.world, target);
    if dealt == 0 {
        state.log.push(if amount > 0 {
            capitalize(&format!("{} armor absorbs the blow.", victim.possessive()))
        } else {
            victim.sentence("be", "unharmed")
        });
        return;
    }

    let remaining = {
        let mut health = state
            .world
            .get::<&mut Health>(target)
            .expect("checked above; handlers cannot remove components");
        health.current -= dealt;
        health.current
    };
    let blocked = amount - dealt;
    state.log.push(if blocked > 0 {
        victim.sentence("take", &format!("{dealt} damage ({blocked} blocked)"))
    } else {
        victim.sentence("take", &format!("{dealt} damage"))
    });

    let _ = dispatch(
        state,
        target,
        Damaged {
            target,
            amount: dealt,
        },
    );
    // Death is queued before anything the hit triggers, so a kill shot's
    // effects on the victim fizzle instead of being announced first.
    // `AfterHit` still fires, keeping combat-stream draws the same.
    if remaining <= 0 {
        state.commands.push_back(Command::Die { entity: target });
    }
    if let DamageCause::Melee { attacker, .. } = cause {
        let _ = dispatch(
            state,
            attacker,
            AfterHit {
                attacker,
                target,
                amount: dealt,
            },
        );
    }
}

/// The player is marked `Dead` and kept, so the frontend can still show
/// them; everything else is despawned with its attachments.
fn die(state: &mut GameState, entity: Entity) {
    if !state.world.contains(entity) {
        return;
    }
    let dying = Subject::of(&state.world, entity);
    state.log.push(dying.sentence("die", ""));
    if entity == state.player {
        state.game_over = true;
        let _ = state.world.insert_one(entity, Dead);
        return;
    }

    let mut doomed = vec![entity];
    while let Some(next) = doomed.pop() {
        doomed.extend(holding::attached(&state.world, next));
        doomed.extend(holding::carried(&state.world, next));
        let _ = state.world.despawn(next);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::entity::{DamageType, Melee};
    use crate::spawn::Spawn;
    use crate::state::tests::bare_state;
    use crate::testkit::{last_log, thing};

    /// For damage tests that don't care where the hit came from; an effect
    /// cause never triggers on-hit effects.
    pub(crate) const UNSOURCED: DamageCause = DamageCause::Effect {
        effect: Entity::DANGLING,
    };

    /// A melee hit from nobody, for tests about armor.
    pub(crate) const BLOW: DamageCause = DamageCause::Melee {
        attacker: Entity::DANGLING,
        types: DamageTypes::single(DamageType::Blunt),
    };

    /// A goblin (8 HP, 1 AV leather armor) east of the player, who hits for
    /// exactly `damage`.
    fn facing_goblin(damage: i32) -> (GameState, Entity, Entity) {
        let mut state = bare_state();
        let goblin = state.spawn(Spawn::blueprint("Goblin").at(2, 1)).unwrap();
        let armor = holding::attached(&state.world, goblin)[0];
        state
            .world
            .insert_one(
                state.player,
                Melee {
                    min: damage,
                    max: damage,
                },
            )
            .unwrap();
        (state, goblin, armor)
    }

    fn bump_east(state: &mut GameState) {
        state.run(Command::TryMove(state.player, Dir::E));
    }

    fn knife_at(state: &mut GameState, x: i32, y: i32) -> Entity {
        thing("knife").item().at(x, y).make(state)
    }

    fn inventory(state: &GameState, entity: Entity) -> Vec<Entity> {
        holding::carried(&state.world, entity)
    }

    fn pick_up_here(state: &mut GameState) {
        state.run(Command::PickUp(state.player));
    }

    #[test]
    fn picking_up_takes_every_item_here_in_spawn_order() {
        let mut state = bare_state();
        let first = knife_at(&mut state, 1, 1);
        let second = knife_at(&mut state, 1, 1);
        let elsewhere = knife_at(&mut state, 2, 1);

        pick_up_here(&mut state);

        assert_eq!(inventory(&state, state.player), [first, second]);
        assert!(state.world.get::<&Position>(first).is_err());
        assert!(state.world.get::<&Position>(elsewhere).is_ok());
        let log: Vec<&String> = state.log.recent(2).collect();
        assert_eq!(log, ["You pick up the knife.", "You pick up the knife."]);
    }

    #[test]
    fn dropping_puts_the_item_at_your_feet_as_found() {
        let mut state = bare_state();
        let knife = knife_at(&mut state, 1, 1);
        pick_up_here(&mut state);
        state.run(Command::Drop {
            actor: state.player,
            item: knife,
        });

        assert!(inventory(&state, state.player).is_empty());
        assert_eq!(
            *state.world.get::<&Position>(knife).unwrap(),
            Position { x: 1, y: 1 }
        );
        assert!(state.world.satisfies::<&Found>(knife));
        assert_eq!(last_log(&state), "You drop the knife.");
    }

    #[test]
    fn dropping_what_you_do_not_carry_changes_nothing() {
        let mut state = bare_state();
        let knife = knife_at(&mut state, 5, 5);
        state.run(Command::Drop {
            actor: state.player,
            item: knife,
        });
        assert_eq!(
            *state.world.get::<&Position>(knife).unwrap(),
            Position { x: 5, y: 5 }
        );
        assert_eq!(last_log(&state), "You do not carry that.");
    }

    #[test]
    fn a_dying_creature_takes_its_inventory_with_it() {
        let mut state = bare_state();
        let goblin = thing("goblin").at(3, 3).make(&mut state);
        let knife = thing("knife").item().carried_by(goblin).make(&mut state);
        state.run(Command::Die { entity: goblin });
        assert!(!state.world.contains(knife));
    }

    #[test]
    fn stepping_onto_an_item_finds_it_once() {
        let mut state = bare_state();
        let knife = knife_at(&mut state, 2, 1);
        bump_east(&mut state);
        assert!(state.world.satisfies::<&Found>(knife));
        assert_eq!(last_log(&state), "You find the knife.");

        state.log.push("marker");
        state.run(Command::FindItems(state.player));
        assert_eq!(
            last_log(&state),
            "marker",
            "a found item is not announced again"
        );
    }

    #[test]
    fn only_the_player_finds_items() {
        let mut state = bare_state();
        let knife = knife_at(&mut state, 3, 1);
        let walker = thing("goblin").at(2, 1).make(&mut state);
        state.run(Command::TryMove(walker, Dir::E));
        assert_eq!(
            *state.world.get::<&Position>(walker).unwrap(),
            Position { x: 3, y: 1 }
        );
        assert!(!state.world.satisfies::<&Found>(knife));
    }

    #[test]
    fn bumping_a_creature_attacks_instead_of_moving() {
        let (mut state, goblin, _) = facing_goblin(4);
        bump_east(&mut state);
        assert_eq!(
            *state.world.get::<&Position>(state.player).unwrap(),
            Position { x: 1, y: 1 }
        );
        assert_eq!(state.world.get::<&Health>(goblin).unwrap().current, 5);
        assert_eq!(last_log(&state), "The goblin takes 3 damage (1 blocked).");
    }

    #[test]
    fn killing_a_creature_despawns_it_and_its_attachments() {
        let (mut state, goblin, armor) = facing_goblin(5);
        bump_east(&mut state);
        assert!(state.world.contains(goblin));
        bump_east(&mut state);
        assert!(!state.world.contains(goblin));
        assert!(!state.world.contains(armor));
        assert_eq!(last_log(&state), "The goblin dies.");
        assert!(state.commands.is_empty());

        // The tile is free now.
        bump_east(&mut state);
        assert_eq!(
            *state.world.get::<&Position>(state.player).unwrap(),
            Position { x: 2, y: 1 }
        );
    }

    #[test]
    fn attack_without_melee_does_nothing() {
        let (mut state, goblin, _) = facing_goblin(4);
        state.world.remove_one::<Melee>(state.player).unwrap();
        bump_east(&mut state);
        assert_eq!(state.world.get::<&Health>(goblin).unwrap().current, 8);
        assert!(state.commands.is_empty());
    }

    #[test]
    fn damage_to_something_without_health_is_ignored() {
        let mut state = bare_state();
        let rock = thing("rock").make(&mut state);
        let log_before = state.log.clone();
        state.run(Command::DealDamage {
            target: rock,
            amount: 5,
            cause: UNSOURCED,
        });
        assert_eq!(state.log, log_before);
        assert!(state.world.contains(rock));
    }

    #[test]
    fn zero_damage_without_armor_leaves_the_target_unharmed() {
        let mut state = bare_state();
        let dummy = thing("dummy").health(3).make(&mut state);
        state.run(Command::DealDamage {
            target: dummy,
            amount: 0,
            cause: UNSOURCED,
        });
        assert_eq!(state.world.get::<&Health>(dummy).unwrap().current, 3);
        assert_eq!(last_log(&state), "The dummy is unharmed.");
    }

    #[test]
    fn messages_about_the_player_use_the_second_person() {
        let mut state = bare_state();
        state.run(Command::DealDamage {
            target: state.player,
            amount: 3,
            cause: UNSOURCED,
        });
        assert_eq!(last_log(&state), "You take 3 damage.");

        state.run(Command::Die {
            entity: state.player,
        });
        assert_eq!(last_log(&state), "You die.");
    }

    #[test]
    fn dying_twice_is_harmless() {
        let (mut state, goblin, _) = facing_goblin(1);
        state.commands.push_back(Command::Die { entity: goblin });
        state.run(Command::Die { entity: goblin });
        assert!(!state.world.contains(goblin));
        assert_eq!(
            state.log.recent(2).collect::<Vec<_>>(),
            ["The goblin dies.", "Seed: 1"]
        );
    }

    #[test]
    fn the_player_dies_once_and_is_kept() {
        let mut state = bare_state();
        for _ in 0..2 {
            state.commands.push_back(Command::Die {
                entity: state.player,
            });
        }
        apply_commands(&mut state);
        assert!(state.game_over);
        assert!(state.world.satisfies::<(&Dead, &Position)>(state.player));
        assert_eq!(
            state.log.recent(2).collect::<Vec<_>>(),
            ["You die.", "Seed: 1"]
        );
    }

    #[test]
    fn damage_queued_after_the_players_death_does_nothing() {
        let mut state = bare_state();
        let player = state.player;
        state.commands.push_back(Command::Die { entity: player });
        state.run(Command::DealDamage {
            target: player,
            amount: 5,
            cause: UNSOURCED,
        });
        assert_eq!(state.world.get::<&Health>(player).unwrap().current, 20);
        assert_eq!(
            state.log.recent(2).collect::<Vec<_>>(),
            ["You die.", "Seed: 1"]
        );
    }

    #[test]
    fn an_attack_from_the_dead_player_does_nothing() {
        let (mut state, goblin, _) = facing_goblin(4);
        let player = state.player;
        state.run(Command::Die { entity: player });
        let (log, combat) = (state.log.clone(), state.rngs.combat.clone());

        state.run(Command::Attack {
            attacker: player,
            target: goblin,
        });
        assert_eq!(state.world.get::<&Health>(goblin).unwrap().current, 8);
        assert_eq!(state.log, log);
        assert_eq!(state.rngs.combat, combat, "no damage was rolled");
        assert!(state.commands.is_empty());
    }

    #[test]
    fn exceeding_the_command_cap_logs_an_error_and_empties_the_queue() {
        let mut state = bare_state();
        for _ in 0..MAX_COMMANDS_PER_STEP + 5 {
            state.commands.push_back(Command::Die {
                entity: Entity::DANGLING,
            });
        }
        apply_commands(&mut state);
        assert!(state.commands.is_empty());
        assert_eq!(
            last_log(&state),
            format!(
                "Error: command limit ({MAX_COMMANDS_PER_STEP}) reached; dropped 5 queued commands."
            )
        );
    }

    #[test]
    fn the_cap_is_not_hit_at_exactly_the_limit() {
        let mut state = bare_state();
        for _ in 0..MAX_COMMANDS_PER_STEP {
            state.commands.push_back(Command::Die {
                entity: Entity::DANGLING,
            });
        }
        apply_commands(&mut state);
        assert_eq!(last_log(&state), "Seed: 1");
    }
}
