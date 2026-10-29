//! `Command` and `apply_commands`: the only place the world is mutated during play (ADR 0002).

use std::ops::ControlFlow;

use hecs::Entity;

use crate::ammo;
use crate::blueprint::BlueprintId;
use crate::combat::{self, AfterHit, BeforeTakeDamage, Damaged};
use crate::cost;
use crate::effect::{self, DurationChange};
use crate::entity::{
    Armor, BlocksMovement, DamageTypes, Dead, Energy, Found, Health, Item, Position, Rider,
    SpawnSeq, Summoned,
};
use crate::equipment;
use crate::event::{Ctx, settle};
use crate::fov;
use crate::fx::{self, Fx};
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
        /// A graze, not a full hit: `AfterHit` says so (ADR 0017).
        graze: bool,
    },
    /// Thrown or fired: armored against like a blow, but not an `AfterHit`
    /// source, so a missile never triggers its thrower's on-hit parts.
    Ranged {
        attacker: Entity,
        types: DamageTypes,
    },
    /// A spell, resisted by armor of its kinds (none for raw force), and not
    /// an `AfterHit` source.
    Spell {
        caster: Entity,
        types: DamageTypes,
    },
    Effect {
        effect: Entity,
    },
}

impl DamageCause {
    /// What armor can resist. Effect damage is of no kind, so it passes.
    pub(crate) fn types(self) -> DamageTypes {
        match self {
            Self::Melee { types, .. } | Self::Ranged { types, .. } | Self::Spell { types, .. } => {
                types
            }
            Self::Effect { .. } => DamageTypes::default(),
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
    /// Perform `technique`, which `actor` knows, at the tile `at` (ADR 0024).
    Perform {
        actor: Entity,
        technique: Entity,
        at: (i32, i32),
    },
    /// What `actor`'s technique does to `target` after a full hit's damage,
    /// if the target lived (`technique::ride`).
    Ride {
        actor: Entity,
        target: Entity,
        rider: Rider,
    },
    /// Start `spell`'s (or a technique's) recharge: it cannot be used for
    /// `duration` ticks.
    Recharge {
        spell: Entity,
        duration: u64,
    },
    /// `actor` studies `item`'s lesson (ADR 0017).
    Study {
        actor: Entity,
        item: Entity,
    },
    /// A turn of drills with what `actor` wields.
    Practice(Entity),
    /// Close an open door beside the actor (`close_door`).
    Close(Entity),
    /// Take the stairs down to the next floor (`dive::descend`).
    Descend(Entity),
    /// Take the stairs up, back to camp (`dive::ascend`).
    Ascend(Entity),
    /// At camp: the next dive will be in phase `phase`, an index into
    /// `Blueprints::phases` (`phase::wait_for`).
    WaitForPhase {
        actor: Entity,
        phase: u32,
    },
    /// Take up a style `actor` knows, or set aside the one they follow
    /// (ADR 0019).
    FollowStyle {
        actor: Entity,
        style: Entity,
    },
    /// Take `amount` from `caster`'s mana pool.
    SpendMana {
        caster: Entity,
        amount: u32,
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
    /// `item`, which `holder` carries, is used up (a read scroll).
    UseUp {
        holder: Entity,
        item: Entity,
    },
    /// A summon leaves: its binding ran out or its owner died. Despawned
    /// with what it holds, like a death but without one.
    Dismiss(Entity),
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
    /// A `Regeneration` holder's tick (`rest::regenerate`).
    Regenerate(Entity),
    /// Scheduler only: `actor` remembers where it last saw an enemy, or
    /// forgets (`None`). Queued before it decides (`ai::notice`).
    Notice {
        actor: Entity,
        at: Option<(i32, i32)>,
    },
    /// Scheduler only: a roamer sets off for `to` (`ai::route`).
    SetRoute {
        actor: Entity,
        to: (i32, i32),
    },
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
            Command::Perform {
                actor, technique, ..
            } => [Some(actor), Some(technique)],
            Command::FollowStyle { actor, style } => [Some(actor), Some(style)],
            Command::Study { actor, item } => [Some(actor), Some(item)],
            Command::Practice(actor)
            | Command::Close(actor)
            | Command::Descend(actor)
            | Command::Ascend(actor)
            | Command::WaitForPhase { actor, .. } => [Some(actor), None],
            Command::Recharge { spell, .. } => [Some(spell), None],
            Command::Ride { actor, target, .. } => [Some(actor), Some(target)],
            Command::SpendMana { caster, .. } => [Some(caster), None],
            Command::Attack { attacker, target } => [Some(attacker), Some(target)],
            Command::DealDamage { target, .. } | Command::ApplyEffect { target, .. } => {
                [Some(target), None]
            }
            Command::Die { entity } => [Some(entity), None],
            Command::Dismiss(entity) => [Some(entity), None],
            Command::UseUp { holder, item } => [Some(holder), Some(item)],
            Command::ModifyDuration { effect, .. }
            | Command::ExpireEffect(effect)
            | Command::BleedTick(effect) => [Some(effect), None],
            Command::Regenerate(holder) => [Some(holder), None],
            Command::Notice { actor, .. } | Command::SetRoute { actor, .. } => [Some(actor), None],
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
            ammo::gather(state, actor);
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
            Command::Perform {
                actor,
                technique,
                at,
            } => crate::technique::perform(state, actor, technique, at),
            Command::FollowStyle { actor, style } => crate::style::follow(state, actor, style),
            Command::Study { actor, item } => crate::training::study(state, actor, item),
            Command::Practice(actor) => crate::training::practice(state, actor),
            Command::Close(actor) => close_door(state, actor),
            Command::Descend(actor) => crate::dive::descend(state, actor),
            Command::Ascend(actor) => crate::dive::ascend(state, actor),
            Command::WaitForPhase { actor, phase } => crate::phase::wait_for(state, actor, phase),
            Command::Recharge { spell, duration } => spell::recharge(state, spell, duration),
            Command::Ride {
                actor,
                target,
                rider,
            } => crate::technique::ride(state, actor, target, rider),
            Command::SpendMana { caster, amount } => spell::spend_mana(state, caster, amount),
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
            Command::Dismiss(entity) => dismiss(state, entity),
            Command::UseUp { holder, item } => use_up(state, holder, item),
            Command::ApplyEffect { target, blueprint } => {
                effect::apply_effect(state, target, blueprint);
            }
            Command::ModifyDuration { effect, change } => {
                effect::modify_duration(state, effect, change);
            }
            Command::ExpireEffect(effect) => effect::expire_effect(state, effect),
            Command::BleedTick(effect) => effect::bleed_tick(state, effect),
            Command::Regenerate(holder) => crate::rest::regenerate(state, holder),
            Command::Notice { actor, at } => crate::ai::remember(state, actor, at),
            Command::SetRoute { actor, to } => {
                let _ = state.world.insert_one(actor, crate::entity::Route { to });
            }
            Command::AdvanceClock { ticks } => time::advance_clock(state, ticks),
            Command::UpdateFov => {
                fov::update(state);
                crate::lore::sight(state);
            }
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
        Command::Close(entity) => {
            if closable(state, entity).is_some() {
                return false;
            }
            state.log.push("There is no open door beside you to close.");
        }
        Command::PickUp(entity) => {
            if !floor_items_under(state, entity).is_empty() {
                return false;
            }
            state.log.push("There is nothing here to pick up.");
        }
        Command::WaitForPhase { phase, .. } => {
            let known = usize::try_from(phase).is_ok_and(|i| i < state.blueprints.phases().len());
            match crate::phase::wait_refusal(state) {
                None if known => return false,
                None => state.log.push("There is no such phase."),
                Some(why) => state.log.push(why),
            }
        }
        Command::Descend(entity) | Command::Ascend(entity) => {
            let (stairs, refusal) = match *command {
                Command::Descend(_) => (Tile::StairsDown, "There are no stairs down here."),
                _ => (Tile::StairsUp, "There are no stairs up here."),
            };
            let on_stairs = state
                .world
                .get::<&Position>(entity)
                .is_ok_and(|at| state.map.get(at.x, at.y) == Some(&stairs));
            if on_stairs {
                return false;
            }
            state.log.push(refusal);
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
        Command::Perform {
            actor,
            technique,
            at,
        } => {
            let Err(message) = crate::technique::check_perform(state, actor, technique, at) else {
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

/// Off-map counts as wall; a closed door is not one (walking in opens it).
fn is_wall(state: &GameState, (x, y): (i32, i32)) -> bool {
    !Tile::is_way(state.map.get(x, y))
}

/// Walking into a closed door opens it, and that is the move.
fn open_door(state: &mut GameState, entity: Entity, (x, y): (i32, i32)) {
    state.map.set(x, y, Tile::DoorOpen);
    if crate::ai::can_see_player(state, entity) || entity == state.player {
        let opener = Subject::of(&state.world, entity);
        state.log.push(opener.sentence("open", "the door"));
    }
}

/// `Command::Close`: the first open door beside `actor` (in `Dir::ALL`
/// order) with nothing standing in it; `reject_input` has checked there is
/// one.
fn close_door(state: &mut GameState, actor: Entity) {
    let Some((x, y)) = closable(state, actor) else {
        return;
    };
    state.map.set(x, y, Tile::Door);
    let closer = Subject::of(&state.world, actor);
    state.log.push(closer.sentence("close", "the door"));
}

/// An open door beside `actor` that nothing stands in.
pub(crate) fn closable(state: &GameState, actor: Entity) -> Option<(i32, i32)> {
    let at = *state.world.get::<&Position>(actor).ok()?;
    Dir::ALL
        .into_iter()
        .map(|dir| {
            let (dx, dy) = dir.delta();
            (at.x + dx, at.y + dy)
        })
        .find(|&(x, y)| {
            state.map.get(x, y) == Some(&Tile::DoorOpen)
                && !state
                    .world
                    .query::<&Position>()
                    .iter()
                    .any(|pos| (pos.x, pos.y) == (x, y))
        })
}

fn log_wall_bump(state: &mut GameState, entity: Entity) {
    let mover = Subject::of(&state.world, entity);
    state.log.push(mover.sentence("bump", "into a wall"));
}

/// `a` and `b` trade tiles.
fn swap_places(state: &mut GameState, a: Entity, b: Entity) {
    let (Ok(at_a), Ok(at_b)) = (
        state.world.get::<&Position>(a).map(|p| *p),
        state.world.get::<&Position>(b).map(|p| *p),
    ) else {
        return;
    };
    let _ = state.world.insert_one(a, at_b);
    let _ = state.world.insert_one(b, at_a);
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
    if state.map.get(target.0, target.1) == Some(&Tile::Door) {
        open_door(state, entity, target);
        return;
    }

    let victim = state
        .world
        .query::<(Entity, &Position, &Health)>()
        .iter()
        .find(|&(other, pos, _)| other != entity && (pos.x, pos.y) == target)
        .map(|(other, _, _)| other);
    if let Some(victim) = victim {
        // Walking into your own summon trades places with it.
        let own = state
            .world
            .get::<&Summoned>(victim)
            .is_ok_and(|summoned| summoned.owner == entity);
        if own {
            swap_places(state, entity, victim);
            return;
        }
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
    let _ = combat::blow(state, attacker, target, &combat::BlowMods::default());
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
        if amount > 0 {
            fx::at(state, target, |at| Fx::Outcome {
                at,
                kind: fx::Outcome::Absorbed,
            });
        }
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
    let to_player = target == state.player;
    fx::at(state, target, |at| Fx::Damage {
        at,
        amount: dealt,
        blocked,
        to_player,
    });
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
    if let DamageCause::Melee {
        attacker, graze, ..
    } = cause
    {
        let _ = dispatch(
            state,
            attacker,
            AfterHit {
                attacker,
                target,
                amount: dealt,
                graze,
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
    fx::at(state, entity, |at| Fx::Death { at });
    let summons: Vec<Entity> = state
        .world
        .query::<(Entity, &Summoned)>()
        .iter()
        .filter(|&(_, summoned)| summoned.owner == entity)
        .map(|(summon, _)| summon)
        .collect();
    for summon in summons {
        state.commands.push_back(Command::Dismiss(summon));
    }
    if entity == state.player {
        state.game_over = true;
        let _ = state.world.insert_one(entity, Dead);
        return;
    }
    despawn_with_holdings(state, entity);
}

/// `Command::UseUp` applier.
fn use_up(state: &mut GameState, holder: Entity, item: Entity) {
    if !holding::carried(&state.world, holder).contains(&item) {
        return;
    }
    let used = Subject::of(&state.world, item);
    state.log.push(used.sentence("crumble", "to dust"));
    holding::unstow(&mut state.world, holder, item);
    despawn_with_holdings(state, item);
}

/// `Command::Dismiss` applier.
fn dismiss(state: &mut GameState, entity: Entity) {
    if !state.world.contains(entity) {
        return;
    }
    let leaving = Subject::of(&state.world, entity);
    state.log.push(leaving.sentence("fade", "away"));
    despawn_with_holdings(state, entity);
}

pub(crate) fn despawn_with_holdings(state: &mut GameState, entity: Entity) {
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
    use crate::testkit::{last_log, sure_hits, thing};

    /// For damage tests that don't care where the hit came from; an effect
    /// cause never triggers on-hit effects.
    pub(crate) const UNSOURCED: DamageCause = DamageCause::Effect {
        effect: Entity::DANGLING,
    };

    /// A melee hit from nobody, for tests about armor.
    pub(crate) const BLOW: DamageCause = DamageCause::Melee {
        attacker: Entity::DANGLING,
        types: DamageTypes::single(DamageType::Blunt),
        graze: false,
    };

    /// A goblin (8 HP, 1 AV leather armor) east of the player, who hits for
    /// exactly `damage`.
    fn facing_goblin(damage: i32) -> (GameState, Entity, Entity) {
        let mut state = bare_state();
        let player = state.player;
        sure_hits(&mut state, player);
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
    fn walking_into_a_closed_door_opens_it_and_takes_the_move() {
        let mut state = bare_state();
        state.map.set(2, 1, Tile::Door);
        let before = state.now;
        crate::time::take_turn(&mut state, crate::input::PlayerAction::Move(Dir::E));
        assert!(state.now > before, "a turn");
        assert_eq!(state.map.get(2, 1), Some(&Tile::DoorOpen));
        assert_eq!(
            *state.world.get::<&Position>(state.player).unwrap(),
            Position { x: 1, y: 1 },
            "it opens, you stay"
        );
        assert_eq!(crate::testkit::last_log(&state), "You open the door.");
    }

    #[test]
    fn a_closed_door_blocks_sight_and_closing_one_needs_it_free() {
        let mut state = bare_state();
        state.map.set(2, 1, Tile::Door);
        assert!(!Tile::is_open(state.map.get(2, 1)));
        assert!(Tile::is_way(state.map.get(2, 1)));
        let before = state.now;
        crate::time::take_turn(&mut state, crate::input::PlayerAction::Close);
        assert_eq!(state.now, before, "nothing open to close");
        assert_eq!(
            crate::testkit::last_log(&state),
            "There is no open door beside you to close."
        );
        state.map.set(2, 1, Tile::DoorOpen);
        crate::time::take_turn(&mut state, crate::input::PlayerAction::Close);
        assert_eq!(state.map.get(2, 1), Some(&Tile::Door));
        assert_eq!(crate::testkit::last_log(&state), "You close the door.");
    }

    #[test]
    fn a_goblin_opens_a_door_on_its_way() {
        let mut state = bare_state();
        state.map.set(2, 1, Tile::Door);
        let goblin = crate::testkit::thing("goblin")
            .health(5)
            .hostile()
            .at(3, 1)
            .make(&mut state);
        state.run(Command::TryMove(goblin, Dir::W));
        assert_eq!(state.map.get(2, 1), Some(&Tile::DoorOpen));
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
    fn blows_and_a_kill_the_player_sees_are_effects() {
        let (mut state, _, _) = facing_goblin(5);
        state.visible.set(2, 1, true);
        bump_east(&mut state);
        bump_east(&mut state);
        let hit = Fx::Damage {
            at: (2, 1),
            amount: 4,
            blocked: 1,
            to_player: false,
        };
        assert_eq!(
            fx::take_fx(&mut state),
            [hit.clone(), hit, Fx::Death { at: (2, 1) }]
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
