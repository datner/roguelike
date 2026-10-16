//! Game time (ADR 0006): the global `Tick` clock, the timer queue, and the
//! energy scheduler that decides who acts next.
//!
//! The scheduler never walks tick by tick: it jumps the clock straight to
//! the next tick where a timer is due or an actor becomes ready. At one tick,
//! due timers fire first (in insertion order), then ready actors act in
//! `SpawnSeq` order. It stops when the player is ready, so the frontend can
//! ask for input.

use std::collections::BTreeMap;

use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::ai;
use crate::command::{Command, apply_commands, reject_input};
use crate::cost::TICKS_PER_TURN;
use crate::entity::{Dead, Energy, SpawnSeq, Speed};
use crate::equipment::equipped;
use crate::input::PlayerAction;
use crate::state::GameState;
use crate::view::carried;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Tick(pub u64);

impl Tick {
    pub fn after(self, ticks: u64) -> Tick {
        Tick(self.0.saturating_add(ticks))
    }

    /// Ticks from `earlier` to `self`; 0 if `earlier` is later.
    pub fn since(self, earlier: Tick) -> u64 {
        self.0.saturating_sub(earlier.0)
    }

    /// The turn number shown to the player.
    pub fn turn(self) -> u64 {
        self.0 / TICKS_PER_TURN
    }
}

/// Something to do at a tick. Timers are idempotent checks against the
/// current state, never trusted as the source of truth: firing one for an
/// entity that is gone, or whose deadline moved, does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Timer {
    ExpireEffect(Entity),
    BleedTick(Entity),
}

impl Timer {
    fn command(self) -> Command {
        match self {
            Timer::ExpireEffect(effect) => Command::ExpireEffect(effect),
            Timer::BleedTick(effect) => Command::BleedTick(effect),
        }
    }
}

/// Timers keyed by (due tick, insertion seq), so timers due at the same tick
/// fire in the order they were scheduled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timers {
    pub(crate) queue: BTreeMap<(Tick, u64), Timer>,
    pub(crate) next_seq: u64,
}

impl Timers {
    pub(crate) fn schedule(&mut self, at: Tick, timer: Timer) {
        self.queue.insert((at, self.next_seq), timer);
        self.next_seq += 1;
    }

    pub fn next_due(&self) -> Option<Tick> {
        self.queue.first_key_value().map(|(&(at, _), _)| at)
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    fn pop_due(&mut self, now: Tick) -> Option<Timer> {
        if self.next_due()? > now {
            return None;
        }
        self.queue.pop_first().map(|(_, timer)| timer)
    }
}

/// What one scheduler step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Timer(Timer),
    Actor(Entity),
    /// The clock jumped to this tick.
    Clock(Tick),
    PlayerReady,
    GameOver,
    /// Nothing is scheduled and nobody can ever act.
    Idle,
}

/// Applies the player's action as a command for `state.player`, recomputes
/// their field of view once it has fully resolved, then runs the world until
/// the player is ready to act again (or the game ends). Input refused by
/// `command::reject_input` does none of that.
pub fn take_turn(state: &mut GameState, action: PlayerAction) {
    let player = state.player;
    let command = match action {
        PlayerAction::Move(dir) => Command::TryMove(player, dir),
        PlayerAction::Wait => Command::Wait(player),
        PlayerAction::PickUp => Command::PickUp(player),
        PlayerAction::Drop(item) => {
            let Some(item) = item.find(&state.world, &carried(state)) else {
                state.log.push("You do not carry that.");
                return;
            };
            Command::Drop {
                actor: player,
                item,
            }
        }
        PlayerAction::Equip { item, part } => {
            let Some(item) = item.find(&state.world, &carried(state)) else {
                state.log.push("You do not carry that.");
                return;
            };
            Command::Equip {
                actor: player,
                item,
                part,
            }
        }
        PlayerAction::Unequip(item) => {
            let worn: Vec<_> = equipped(&state.world, player)
                .into_iter()
                .map(|(item, _)| item)
                .collect();
            let Some(item) = item.find(&state.world, &worn) else {
                state.log.push("You do not have that equipped.");
                return;
            };
            Command::Unequip {
                actor: player,
                item,
            }
        }
    };
    if state.game_over || reject_input(state, &command) {
        return;
    }
    state.commands.push_back(command);
    apply_commands(state);
    state.commands.push_back(Command::UpdateFov);
    apply_commands(state);
    advance(state);
}

/// Runs timers and non-player actors until the player is ready, the game is
/// over, or nothing is left to happen.
pub fn advance(state: &mut GameState) {
    while !matches!(step(state), Step::PlayerReady | Step::GameOver | Step::Idle) {}
}

pub(crate) fn step(state: &mut GameState) -> Step {
    if state.game_over {
        return Step::GameOver;
    }
    if let Some(timer) = state.timers.pop_due(state.now) {
        state.commands.push_back(timer.command());
        apply_commands(state);
        return Step::Timer(timer);
    }
    if let Some(actor) = next_ready(state) {
        if actor == state.player {
            return Step::PlayerReady;
        }
        act(state, actor);
        return Step::Actor(actor);
    }
    match ticks_until_next(state) {
        Some(ticks) => {
            state.commands.push_back(Command::AdvanceClock { ticks });
            apply_commands(state);
            Step::Clock(state.now)
        }
        None => Step::Idle,
    }
}

/// The ready actor (energy >= 0) that spawned first. `Dead` actors are
/// never scheduled.
fn next_ready(state: &GameState) -> Option<Entity> {
    state
        .world
        .query::<(Entity, &Energy, &SpawnSeq)>()
        .without::<&Dead>()
        .iter()
        .filter(|(_, energy, _)| energy.0 >= 0)
        .min_by_key(|&(_, _, seq)| *seq)
        .map(|(entity, _, _)| entity)
}

fn energy(state: &GameState, actor: Entity) -> Option<i64> {
    state.world.get::<&Energy>(actor).ok().map(|e| e.0)
}

fn act(state: &mut GameState, actor: Entity) {
    let before = energy(state, actor);
    let command = ai::decide(state, actor);
    state.commands.push_back(command);
    apply_commands(state);
    // Every AI decision costs energy; if a bug ever breaks that, waiting
    // keeps the scheduler from spinning on a forever-ready actor.
    if before.is_some() && energy(state, actor) >= before {
        state.commands.push_back(Command::Wait(actor));
        apply_commands(state);
    }
}

/// Ticks until the next timer is due or an actor becomes ready. Only called
/// when nothing is due and nobody is ready now, so the result is > 0.
fn ticks_until_next(state: &GameState) -> Option<u64> {
    let timer = state.timers.next_due().map(|at| at.since(state.now));
    let actor = state
        .world
        .query::<(&Speed, &Energy)>()
        .without::<&Dead>()
        .iter()
        .filter(|(speed, _)| speed.0 > 0)
        .map(|(speed, energy)| energy.0.unsigned_abs().div_ceil(u64::from(speed.0)))
        .min();
    timer.into_iter().chain(actor).min()
}

/// `AdvanceClock` applier: moves the clock and grants every actor
/// `speed × ticks` energy.
pub(crate) fn advance_clock(state: &mut GameState, ticks: u64) {
    state.now = state.now.after(ticks);
    let ticks = i64::try_from(ticks).unwrap_or(i64::MAX);
    for (speed, energy) in state.world.query_mut::<(&Speed, &mut Energy)>() {
        energy.0 = energy
            .0
            .saturating_add(i64::from(speed.0).saturating_mul(ticks));
    }
}

#[cfg(test)]
mod tests {
    use std::ops::ControlFlow;

    use super::*;
    use crate::combat::Damaged;
    use crate::command::MAX_COMMANDS_PER_STEP;
    use crate::command::tests::UNSOURCED;
    use crate::cost;
    use crate::entity::{Health, Hostile, Melee, Position};
    use crate::input::{Dir, InputEvent};
    use crate::state::tests::{bare_state, spawn_with, test_blueprints};
    use crate::zone::tests::test_cave;

    /// A state whose player is not scheduled, so `step` never stops for input.
    fn unscheduled_player() -> GameState {
        let mut state = bare_state();
        state.world.remove_one::<Energy>(state.player).unwrap();
        state
    }

    fn actor(state: &mut GameState, speed: u32, energy: i64, seq: u64) -> Entity {
        spawn_with(state, (Speed(speed), Energy(energy), SpawnSeq(seq)))
    }

    fn steps(state: &mut GameState, n: usize) -> Vec<Step> {
        (0..n).map(|_| step(state)).collect()
    }

    fn actors_in_order(state: &mut GameState, n: usize) -> Vec<Entity> {
        let mut acted = Vec::new();
        while acted.len() < n {
            if let Step::Actor(actor) = step(state) {
                acted.push(actor);
            }
        }
        acted
    }

    #[test]
    fn a_double_speed_actor_acts_twice_per_normal_action() {
        let mut state = unscheduled_player();
        let fast = actor(&mut state, 200, 0, 1);
        let slow = actor(&mut state, 100, 0, 2);

        let order = actors_in_order(&mut state, 2 + 3 * 9);
        assert_eq!(order[..2], [fast, slow], "both ready at tick 0");
        // After the first round, every slow action is preceded by exactly
        // two fast ones.
        for window in order[2..].chunks(3) {
            assert_eq!(window, [fast, fast, slow]);
        }
        assert_eq!(state.now, Tick(900));
    }

    #[test]
    fn equally_ready_actors_act_in_spawn_seq_order_not_entity_order() {
        let mut state = unscheduled_player();
        let spawned_first = actor(&mut state, 100, 0, 9);
        let spawned_second = actor(&mut state, 100, 0, 3);
        assert_eq!(
            actors_in_order(&mut state, 4),
            [spawned_second, spawned_first, spawned_second, spawned_first]
        );
    }

    #[test]
    fn the_clock_jumps_straight_to_the_next_ready_actor() {
        let mut state = unscheduled_player();
        let slow = actor(&mut state, 30, -cost::STANDARD, 1);
        // 10 000 energy at 30 per tick: ready after ceil(333.3) ticks.
        assert_eq!(
            steps(&mut state, 2),
            [Step::Clock(Tick(334)), Step::Actor(slow)]
        );
    }

    #[test]
    fn same_tick_timers_fire_in_insertion_order_before_actors() {
        let mut state = unscheduled_player();
        let ready_at_100 = actor(&mut state, 100, -cost::STANDARD, 0);
        let timers = [
            Timer::BleedTick(Entity::DANGLING),
            Timer::ExpireEffect(Entity::DANGLING),
            Timer::BleedTick(Entity::DANGLING),
        ];
        for timer in timers {
            state.timers.schedule(Tick(100), timer);
        }
        assert_eq!(
            steps(&mut state, 5),
            [
                Step::Clock(Tick(100)),
                Step::Timer(timers[0]),
                Step::Timer(timers[1]),
                Step::Timer(timers[2]),
                Step::Actor(ready_at_100),
            ]
        );
    }

    #[test]
    fn the_scheduler_stops_when_the_player_is_ready() {
        let mut state = bare_state();
        assert_eq!(step(&mut state), Step::PlayerReady);

        take_turn(&mut state, PlayerAction::Wait);
        assert_eq!(state.now, Tick(100));
        assert_eq!(state.now.turn(), 1);
        assert_eq!(state.world.get::<&Energy>(state.player).unwrap().0, 0);
    }

    #[test]
    fn a_bump_attack_costs_one_standard_action() {
        let mut state = bare_state();
        let dummy = spawn_with(
            &mut state,
            (
                Position { x: 2, y: 1 },
                Health {
                    current: 100,
                    max: 100,
                },
            ),
        );
        state
            .commands
            .push_back(Command::TryMove(state.player, Dir::E));
        apply_commands(&mut state);
        assert!(state.world.contains(dummy));
        assert_eq!(
            state.world.get::<&Energy>(state.player).unwrap().0,
            -cost::STANDARD
        );
    }

    #[test]
    fn nothing_scheduled_is_idle() {
        let mut state = unscheduled_player();
        assert_eq!(step(&mut state), Step::Idle);
    }

    #[test]
    fn a_dead_player_is_kept_but_never_scheduled() {
        let mut state = bare_state();
        let player = state.player;
        state.commands.push_back(Command::Die { entity: player });
        apply_commands(&mut state);

        assert!(state.game_over);
        assert!(state.world.satisfies::<&Dead>(player));
        assert_eq!(state.world.get::<&Energy>(player).unwrap().0, 0);
        assert_eq!(next_ready(&state), None);
        assert_eq!(ticks_until_next(&state), None);
        assert_eq!(step(&mut state), Step::GameOver);

        let view = crate::state::render_view(&state, 40, 20);
        assert_eq!(view.get(1, 1).map(|cell| cell.glyph), Some('@'));
    }

    type ActorState = (Entity, Option<Energy>, Option<Position>, Option<Health>);

    /// Every entity's scheduler-visible state, for before/after comparisons.
    fn snapshot(state: &GameState) -> Vec<ActorState> {
        state
            .world
            .query::<(Entity, Option<&Energy>, Option<&Position>, Option<&Health>)>()
            .iter()
            .map(|(e, energy, pos, health)| (e, energy.copied(), pos.copied(), health.copied()))
            .collect()
    }

    #[test]
    fn walking_into_a_wall_is_refused_without_spending_time() {
        let mut state = bare_state();
        advance(&mut state);
        let (now, before) = (state.now, snapshot(&state));
        // (1,1) -> north is (1,0), a wall in the test room.
        take_turn(&mut state, PlayerAction::Move(Dir::N));

        assert_eq!(state.now, now);
        assert_eq!(snapshot(&state), before);
        assert!(state.commands.is_empty());
        assert_eq!(
            state.log.recent(1).next().map(String::as_str),
            Some("You bump into a wall.")
        );
    }

    fn logged_an_error(state: &GameState) -> bool {
        state.log.recent(usize::MAX).any(|m| m.starts_with("Error"))
    }

    /// `per_hit` extra no-op commands for every hit landed, to make each
    /// attack step as long as the test needs.
    fn pad_hits(state: &mut GameState, per_hit: usize) {
        state
            .events
            .on::<Damaged, Health>(i32::MAX, move |ctx, _, ev| {
                ctx.out
                    .extend((0..per_hit).map(|_| Command::Wait(Entity::DANGLING)));
                ControlFlow::Continue(ev)
            });
    }

    #[test]
    fn a_crowd_acting_between_player_turns_does_not_share_one_budget() {
        const HOSTILES: u64 = 300;
        let mut state = bare_state();
        let player = state.player;
        state
            .world
            .insert_one(
                player,
                Health {
                    current: 1000,
                    max: 1000,
                },
            )
            .unwrap();
        // TryMove, Attack, DealDamage and one pad per hostile's action.
        pad_hits(&mut state, 1);
        assert!(4 * HOSTILES as usize > MAX_COMMANDS_PER_STEP);
        for seq in 1..=HOSTILES {
            spawn_with(
                &mut state,
                (
                    Position { x: 2, y: 1 },
                    Hostile,
                    Melee { min: 1, max: 1 },
                    Speed(100),
                    Energy(0),
                    SpawnSeq(seq),
                ),
            );
        }

        take_turn(&mut state, PlayerAction::Wait);

        assert!(!logged_an_error(&state));
        assert_eq!(state.world.get::<&Health>(player).unwrap().current, 700);
        assert_eq!(step(&mut state), Step::PlayerReady);
    }

    #[test]
    fn a_self_feeding_loop_within_one_step_still_hits_the_cap() {
        let mut state = unscheduled_player();
        let player = state.player;
        // Every hit queues another hit: an endless chain inside one action.
        state.events.on::<Damaged, Health>(i32::MAX, |ctx, _, ev| {
            ctx.out.push(Command::DealDamage {
                target: ev.target,
                amount: 1,
                cause: UNSOURCED,
            });
            ControlFlow::Continue(ev)
        });
        state
            .world
            .insert_one(
                player,
                Health {
                    current: i32::MAX,
                    max: i32::MAX,
                },
            )
            .unwrap();
        let hostile = spawn_with(
            &mut state,
            (
                Position { x: 2, y: 1 },
                Hostile,
                Melee { min: 1, max: 1 },
                Speed(100),
                Energy(0),
                SpawnSeq(1),
            ),
        );

        assert_eq!(step(&mut state), Step::Actor(hostile));
        assert!(logged_an_error(&state));
        assert!(state.commands.is_empty());
    }

    fn play(seed: u64, script: &[InputEvent]) -> GameState {
        let mut state = GameState::new(seed, test_blueprints(), &test_cave()).unwrap();
        advance(&mut state);
        for &input in script {
            let action = match input {
                InputEvent::Move(dir) => PlayerAction::Move(dir),
                InputEvent::Wait => PlayerAction::Wait,
                InputEvent::PickUp => PlayerAction::PickUp,
                InputEvent::Inventory | InputEvent::Equipment => continue,
                InputEvent::CycleExploreItems => {
                    crate::explore::cycle_explore_items(&mut state);
                    continue;
                }
                InputEvent::Explore => {
                    while crate::explore::explore_step(&mut state) {}
                    continue;
                }
                InputEvent::Quit => break,
            };
            take_turn(&mut state, action);
        }
        state
    }

    #[test]
    fn replaying_the_same_seed_and_inputs_gives_the_same_game() {
        let mut script = Vec::new();
        for _ in 0..6 {
            script.extend([
                InputEvent::Move(Dir::E),
                InputEvent::Move(Dir::SE),
                InputEvent::Wait,
                InputEvent::Move(Dir::S),
            ]);
        }
        script.push(InputEvent::Explore);
        let a = play(7, &script);
        let b = play(7, &script);
        assert_eq!(a.log, b.log);
        assert_eq!(a.now, b.now);
        assert!(a.now > Tick(0));
    }
}
