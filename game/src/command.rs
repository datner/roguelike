use std::ops::ControlFlow;

use hecs::Entity;

use crate::combat::{BeforeTakeDamage, Damaged, roll_melee};
use crate::cost;
use crate::effect::{self, DurationChange};
use crate::entity::{Attachments, BlocksMovement, Dead, Energy, Health, Position};
use crate::event::{Ctx, settle};
use crate::fov;
use crate::grammar::{Subject, capitalize};
use crate::input::Dir;
use crate::map::Tile;
use crate::state::GameState;
use crate::time;

/// Upper bound on commands applied per `apply_commands` call, so a handler
/// feedback loop degrades into an error message instead of a hang. Each
/// scheduler step (one action, one timer firing, one clock jump) drains the
/// queue in its own call, so the budget is per step: a crowd of actors
/// between two player turns never shares it.
pub const MAX_COMMANDS_PER_STEP: usize = 1000;

/// Actions (`TryMove`, `Wait`) cost their actor energy; see `cost::of`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    TryMove(Entity, Dir),
    /// Do nothing for a standard action.
    Wait(Entity),
    Attack {
        attacker: Entity,
        target: Entity,
    },
    DealDamage {
        target: Entity,
        amount: i32,
    },
    Die {
        entity: Entity,
    },
    /// Spawn the effect `blueprint` (which must carry a `Duration`), start it
    /// now, and attach it to `target`.
    ApplyEffect {
        target: Entity,
        blueprint: String,
    },
    /// The only way an effect's duration changes (ADR 0006).
    ModifyDuration {
        effect: Entity,
        change: DurationChange,
    },
    ExpireEffect(Entity),
    BleedTick(Entity),
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
            Command::TryMove(actor, _) | Command::Wait(actor) => [Some(actor), None],
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

/// Drains the command queue. This is the only code that mutates the world.
pub fn apply_commands(state: &mut GameState) {
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
            Command::Attack { attacker, target } => attack(state, attacker, target),
            Command::DealDamage { target, amount } => deal_damage(state, target, amount),
            Command::Die { entity } => die(state, entity),
            Command::ApplyEffect { target, blueprint } => {
                effect::apply_effect(state, target, &blueprint);
            }
            Command::ModifyDuration { effect, change } => {
                effect::modify_duration(state, effect, change);
            }
            Command::ExpireEffect(effect) => effect::expire_effect(state, effect),
            Command::BleedTick(effect) => effect::bleed_tick(state, effect),
            Command::AdvanceClock { ticks } => time::advance_clock(state, ticks),
            Command::UpdateFov => fov::update(state),
        }
    }
}

/// Fires `ev` at `target` and queues whatever commands the handlers emitted.
fn dispatch<E: 'static>(state: &mut GameState, target: Entity, ev: E) -> ControlFlow<E, E> {
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
/// wall (any wall, seen or not, for now) is refused with a
/// message: nothing is queued, no energy is spent and no time passes.
/// Returns whether the input was refused.
pub fn reject_input(state: &mut GameState, command: &Command) -> bool {
    let &Command::TryMove(entity, dir) = command else {
        return false;
    };
    if !destination(state, entity, dir).is_some_and(|to| is_wall(state, to)) {
        return false;
    }
    log_wall_bump(state, entity);
    true
}

fn destination(state: &GameState, entity: Entity, dir: Dir) -> Option<(i32, i32)> {
    let (dx, dy) = dir.delta();
    let current = state.world.get::<&Position>(entity).ok()?;
    Some((current.x + dx, current.y + dy))
}

/// Off-map counts as wall.
fn is_wall(state: &GameState, (x, y): (i32, i32)) -> bool {
    !matches!(state.map.get(x, y), Some(Tile::Floor))
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
}

fn attack(state: &mut GameState, attacker: Entity, target: Entity) {
    if let Some(amount) = roll_melee(&state.world, &mut state.rngs.combat, attacker) {
        state
            .commands
            .push_back(Command::DealDamage { target, amount });
    }
}

fn deal_damage(state: &mut GameState, target: Entity, amount: i32) {
    if !state.world.satisfies::<&Health>(target) {
        return;
    }

    let dealt = settle(dispatch(state, target, BeforeTakeDamage { target, amount }))
        .amount
        .max(0);
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
    if remaining <= 0 {
        state.commands.push_back(Command::Die { entity: target });
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
        if let Ok(attachments) = state.world.get::<&Attachments>(next) {
            doomed.extend(attachments.0.iter().copied());
        }
        let _ = state.world.despawn(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Melee, Name};
    use crate::state::tests::bare_state;

    fn last_log(state: &GameState) -> &str {
        state
            .log
            .recent(1)
            .next()
            .map(String::as_str)
            .unwrap_or_default()
    }

    /// A goblin (8 HP, 1 AV leather armor) east of the player, who hits for
    /// exactly `damage`.
    fn facing_goblin(damage: i32) -> (GameState, Entity, Entity) {
        let mut state = bare_state();
        let goblin = state
            .blueprints
            .spawn_at(&mut state.world, "Goblin", Position { x: 2, y: 1 })
            .unwrap();
        let armor = state.world.get::<&Attachments>(goblin).unwrap().0[0];
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
        state
            .commands
            .push_back(Command::TryMove(state.player, Dir::E));
        apply_commands(state);
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
        let rock = state.world.spawn((Name("rock".into()),));
        let log_before = state.log.clone();
        state.commands.push_back(Command::DealDamage {
            target: rock,
            amount: 5,
        });
        apply_commands(&mut state);
        assert_eq!(state.log, log_before);
        assert!(state.world.contains(rock));
    }

    #[test]
    fn zero_damage_without_armor_leaves_the_target_unharmed() {
        let mut state = bare_state();
        let dummy = state
            .world
            .spawn((Name("dummy".into()), Health { current: 3, max: 3 }));
        state.commands.push_back(Command::DealDamage {
            target: dummy,
            amount: 0,
        });
        apply_commands(&mut state);
        assert_eq!(state.world.get::<&Health>(dummy).unwrap().current, 3);
        assert_eq!(last_log(&state), "The dummy is unharmed.");
    }

    #[test]
    fn messages_about_the_player_use_the_second_person() {
        let mut state = bare_state();
        state.commands.push_back(Command::DealDamage {
            target: state.player,
            amount: 3,
        });
        apply_commands(&mut state);
        assert_eq!(last_log(&state), "You take 3 damage.");

        state.commands.push_back(Command::Die {
            entity: state.player,
        });
        apply_commands(&mut state);
        assert_eq!(last_log(&state), "You die.");
    }

    #[test]
    fn dying_twice_is_harmless() {
        let (mut state, goblin, _) = facing_goblin(1);
        state.commands.push_back(Command::Die { entity: goblin });
        state.commands.push_back(Command::Die { entity: goblin });
        apply_commands(&mut state);
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
        state.commands.push_back(Command::DealDamage {
            target: player,
            amount: 5,
        });
        apply_commands(&mut state);
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
        state.commands.push_back(Command::Die { entity: player });
        apply_commands(&mut state);
        let (log, combat) = (state.log.clone(), state.rngs.combat.clone());

        state.commands.push_back(Command::Attack {
            attacker: player,
            target: goblin,
        });
        apply_commands(&mut state);
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
