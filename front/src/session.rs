//! A game in progress as a frontend drives it: the state, which mode takes the keys (play, a list, a cursor), auto-explore and practice runs, and the status line. The rules of the keyboard flows live here once, so every frontend behaves the same; frontends only map keys and draw.

use game::{
    FireStep, Fx, GameState, InputEvent, PlayerAction, Shot, cycle_explore_items, explore_step,
    fire_step, phases, practice_step, rest_step, spells, take_fx, take_turn, techniques,
    throwables, travel_step, wait_refusal,
};

use crate::input::Input;
use crate::overlay::{Overlay, OverlayAction, Tab, Views};
use crate::targeting::{self, AimAction, Aiming, Chooser, ChooserAction, LookAction, Looking};

/// What takes the keys, and what is drawn over the map.
#[derive(Debug)]
pub enum Mode {
    Play,
    Overlay(Overlay),
    /// Picking what to throw.
    Choose(Chooser),
    /// Picking a spell to cast.
    ChooseSpell(Chooser),
    /// Picking a technique to perform.
    ChooseTechnique(Chooser),
    /// At camp: picking the phase to wait for.
    ChoosePhase(Chooser),
    Aim(Aiming),
    /// Looking around (`;`).
    Look(Looking),
}

/// Which of a frontend's key maps applies now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keys {
    /// `InputEvent`s: moving and opening modes.
    Game,
    /// `OverlayInput`s: a list is up.
    List,
    /// `AimInput`s: a cursor is on the map.
    Cursor,
    /// Any key: the game is over.
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    /// The player quit, or acknowledged their death: save and leave.
    Quit,
}

/// A run of turns the player started and any key stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auto {
    Explore,
    Practice,
    Rest,
    /// Walking to a known tile.
    Travel((i32, i32)),
}

pub struct Session {
    state: GameState,
    character: String,
    mode: Mode,
    auto: Option<Auto>,
}

impl Session {
    pub fn new(state: GameState, character: impl Into<String>) -> Self {
        Self {
            state,
            character: character.into(),
            mode: Mode::Play,
            auto: None,
        }
    }

    pub fn state(&self) -> &GameState {
        &self.state
    }

    pub fn character(&self) -> &str {
        &self.character
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    pub fn keys(&self) -> Keys {
        if self.state.game_over {
            return Keys::Any;
        }
        match self.mode {
            Mode::Play => Keys::Game,
            Mode::Overlay(_)
            | Mode::Choose(_)
            | Mode::ChooseSpell(_)
            | Mode::ChooseTechnique(_)
            | Mode::ChoosePhase(_) => Keys::List,
            Mode::Aim(_) | Mode::Look(_) => Keys::Cursor,
        }
    }

    /// The run in progress, if any. A frontend calls `auto_step` at its own
    /// pace while this is set, so the walk is visible.
    pub fn auto(&self) -> Option<Auto> {
        self.auto
    }

    /// One turn of the run in progress; `false` once it is over.
    pub fn auto_step(&mut self) -> bool {
        let going = match self.auto {
            Some(Auto::Explore) => explore_step(&mut self.state),
            Some(Auto::Practice) => practice_step(&mut self.state),
            Some(Auto::Rest) => rest_step(&mut self.state),
            Some(Auto::Travel(to)) => travel_step(&mut self.state, to),
            None => false,
        };
        if !going || self.state.game_over {
            self.auto = None;
        }
        self.auto.is_some()
    }

    pub fn stop_auto(&mut self) {
        self.auto = None;
    }

    /// The effects since the last call (flights, damage, deaths), for the
    /// frontend to animate. The world has already moved on.
    pub fn take_fx(&mut self) -> Vec<Fx> {
        take_fx(&mut self.state)
    }

    /// Any input during a run only stops it.
    pub fn handle(&mut self, input: Input) -> Outcome {
        if self.auto.take().is_some() {
            return Outcome::Continue;
        }
        if self.state.game_over {
            return Outcome::Quit;
        }
        let outcome = match input {
            Input::Game(event) => self.handle_game(event),
            Input::List(input) => {
                self.handle_list(input);
                Outcome::Continue
            }
            Input::Cursor(input) => {
                self.handle_cursor(input);
                Outcome::Continue
            }
            Input::TravelTo(to) => {
                if matches!(self.mode, Mode::Play) {
                    self.auto = Some(Auto::Travel(to));
                }
                Outcome::Continue
            }
            Input::Any => Outcome::Continue,
        };
        if self.state.game_over {
            self.mode = Mode::Play;
        }
        outcome
    }

    fn handle_game(&mut self, event: InputEvent) -> Outcome {
        if !matches!(self.mode, Mode::Play) {
            return Outcome::Continue;
        }
        let state = &mut self.state;
        match event {
            InputEvent::Quit => return Outcome::Quit,
            InputEvent::Move(dir) => take_turn(state, PlayerAction::Move(dir)),
            InputEvent::Wait => take_turn(state, PlayerAction::Wait),
            InputEvent::PickUp => take_turn(state, PlayerAction::PickUp),
            InputEvent::Descend => take_turn(state, PlayerAction::Descend),
            InputEvent::Ascend => take_turn(state, PlayerAction::Ascend),
            InputEvent::Explore => self.auto = Some(Auto::Explore),
            InputEvent::Practice => self.auto = Some(Auto::Practice),
            InputEvent::Rest => self.auto = Some(Auto::Rest),
            InputEvent::Look => {
                if let Some(looking) = Looking::new(state) {
                    self.mode = Mode::Look(looking);
                }
            }
            InputEvent::CycleExploreItems => cycle_explore_items(state),
            InputEvent::Inventory => self.mode = Mode::Overlay(Overlay::new(Tab::Inventory)),
            InputEvent::Equipment => self.mode = Mode::Overlay(Overlay::new(Tab::Equipment)),
            InputEvent::Skills => self.mode = Mode::Overlay(Overlay::new(Tab::Skills)),
            InputEvent::Styles => self.mode = Mode::Overlay(Overlay::new(Tab::Styles)),
            InputEvent::Throw => self.mode = start_throw(state),
            InputEvent::Fire => self.mode = start_fire(state),
            InputEvent::Cast => self.mode = start_cast(state),
            InputEvent::Techniques => self.mode = start_techniques(state),
            InputEvent::WaitForPhase => self.mode = start_waiting(state),
        }
        Outcome::Continue
    }

    fn handle_list(&mut self, input: crate::input::OverlayInput) {
        let state = &mut self.state;
        match &mut self.mode {
            Mode::Overlay(overlay) => {
                let views = Views::of(state);
                match overlay.handle(input, &views.contents()) {
                    Some(OverlayAction::Close) => self.mode = Mode::Play,
                    Some(OverlayAction::Act(action)) => take_turn(state, action),
                    None => {}
                }
            }
            Mode::Choose(chooser) => {
                let items: Vec<_> = throwables(state).iter().map(|i| i.item).collect();
                match chooser.handle(input, &items) {
                    Some(ChooserAction::Cancel) => self.mode = Mode::Play,
                    Some(ChooserAction::Pick(item)) => {
                        self.mode = aim_or_play(state, Shot::Throw(item));
                    }
                    None => {}
                }
            }
            Mode::ChooseSpell(chooser) => {
                let listed = spells(state);
                let refs: Vec<_> = listed.iter().map(|spell| spell.spell).collect();
                match chooser.handle(input, &refs) {
                    Some(ChooserAction::Cancel) => self.mode = Mode::Play,
                    Some(ChooserAction::Pick(spell)) => {
                        // A spell that can't be cast keeps the list up,
                        // with the reason in the log.
                        if let Some(aiming) = Aiming::new(state, Shot::Cast(spell)) {
                            self.mode = Mode::Aim(aiming);
                        } else if let Some(why) = listed
                            .into_iter()
                            .find(|listed| listed.spell == spell)
                            .and_then(|listed| listed.preview.blocked)
                        {
                            state.log.push(why);
                        }
                    }
                    None => {}
                }
            }
            Mode::ChooseTechnique(chooser) => {
                let listed = techniques(state);
                let refs: Vec<_> = listed.iter().map(|listed| listed.technique).collect();
                match chooser.handle(input, &refs) {
                    Some(ChooserAction::Cancel) => self.mode = Mode::Play,
                    Some(ChooserAction::Pick(technique)) => {
                        // As with spells: one that can't be performed keeps
                        // the list up, with the reason in the log.
                        if let Some(aiming) = Aiming::new(state, Shot::Technique(technique)) {
                            self.mode = Mode::Aim(aiming);
                        } else if let Some(why) = listed
                            .into_iter()
                            .find(|listed| listed.technique == technique)
                            .and_then(|listed| listed.blocked)
                        {
                            state.log.push(why);
                        }
                    }
                    None => {}
                }
            }
            Mode::ChoosePhase(chooser) => {
                let indices: Vec<u32> = (0..).take(phases(state).len()).collect();
                match chooser.handle(input, &indices) {
                    Some(ChooserAction::Cancel) => self.mode = Mode::Play,
                    Some(ChooserAction::Pick(phase)) => {
                        take_turn(state, PlayerAction::WaitForPhase(phase));
                        self.mode = Mode::Play;
                    }
                    None => {}
                }
            }
            Mode::Play | Mode::Aim(_) | Mode::Look(_) => {}
        }
    }

    fn handle_cursor(&mut self, input: crate::input::AimInput) {
        let state = &mut self.state;
        match &mut self.mode {
            Mode::Look(looking) => match looking.handle(input, state) {
                Some(LookAction::Done) => self.mode = Mode::Play,
                Some(LookAction::Travel(to)) => {
                    self.mode = Mode::Play;
                    self.auto = Some(Auto::Travel(to));
                }
                None => {}
            },
            Mode::Aim(aiming) => match aiming.handle(input, state) {
                Some(AimAction::Cancel) => self.mode = Mode::Play,
                Some(AimAction::Take(action)) => {
                    // A refused throw (out of range, unseen) keeps the cursor
                    // up, with the reason in the log, so it can be nudged.
                    let before = state.now;
                    take_turn(state, action);
                    if state.now != before || state.game_over {
                        self.mode = Mode::Play;
                    }
                }
                None => {}
            },
            Mode::Play
            | Mode::Overlay(_)
            | Mode::Choose(_)
            | Mode::ChooseSpell(_)
            | Mode::ChooseTechnique(_)
            | Mode::ChoosePhase(_) => {}
        }
    }

    /// The status line: what an aimed shot would do, what is under the look
    /// cursor, the death notice, or the seed and turn.
    pub fn status(&self) -> String {
        let state = &self.state;
        match &self.mode {
            Mode::Aim(aiming) => targeting::status(state, aiming),
            Mode::Look(looking) => targeting::look_status(state, looking),
            _ if state.game_over => format!(
                "You died on turn {}. Press any key to return to the title.",
                state.now.turn()
            ),
            _ => format!(
                "{}{}{}  Turn: {}  Seed: {}",
                crate::text::place(state),
                crate::text::rest(state),
                crate::text::outnumbered(state),
                state.now.turn(),
                state.seed
            ),
        }
    }
}

fn aim_or_play(state: &GameState, shot: Shot) -> Mode {
    Aiming::new(state, shot).map_or(Mode::Play, Mode::Aim)
}

/// Pressing fire: aim if an arrow is ready, nock one first (a turn) if only
/// the pack has them, otherwise say why not.
fn start_fire(state: &mut GameState) -> Mode {
    match fire_step(state) {
        FireStep::Aim => aim_or_play(state, Shot::Fire),
        FireStep::Nock(arrow) => {
            take_turn(state, PlayerAction::Nock(arrow));
            Mode::Play
        }
        FireStep::Cannot(why) => {
            state.log.push(why);
            Mode::Play
        }
    }
}

/// Straight to aiming with the only throwable; a list if there are several;
/// a log line if there is none.
fn start_throw(state: &mut GameState) -> Mode {
    let mut options = throwables(state);
    match options.len() {
        0 => {
            state.log.push("You have nothing to throw.");
            Mode::Play
        }
        1 => aim_or_play(state, Shot::Throw(options.remove(0).item)),
        _ => Mode::Choose(Chooser::default()),
    }
}

/// The spell list, even for one spell, since it shows what each costs or why
/// it can't be cast; a log line if the player knows none.
fn start_cast(state: &mut GameState) -> Mode {
    if spells(state).is_empty() {
        state.log.push("You know no spells.");
        return Mode::Play;
    }
    Mode::ChooseSpell(Chooser::default())
}

/// The technique list, like the spell list; a log line if the player knows
/// none.
fn start_techniques(state: &mut GameState) -> Mode {
    if techniques(state).is_empty() {
        state.log.push("You know no techniques.");
        return Mode::Play;
    }
    Mode::ChooseTechnique(Chooser::default())
}

/// The phase list at camp, starting on the phase already chosen; elsewhere
/// a log line saying why not.
fn start_waiting(state: &mut GameState) -> Mode {
    if let Some(why) = wait_refusal(state) {
        state.log.push(why);
        return Mode::Play;
    }
    let mut chooser = Chooser::default();
    let chosen = phases(state).iter().position(|phase| phase.chosen);
    let indices: Vec<u32> = (0..).take(phases(state).len()).collect();
    chooser.handle(
        crate::input::OverlayInput::Focus(chosen.unwrap_or(0)),
        &indices,
    );
    Mode::ChoosePhase(chooser)
}

#[cfg(test)]
mod tests {
    use game::{Dir, Tick};

    use super::*;
    use crate::input::{AimInput, OverlayInput};
    use crate::tests::state;

    fn session() -> Session {
        Session::new(state(), "hero")
    }

    fn last_log(session: &Session) -> String {
        session
            .state()
            .log
            .recent(1)
            .next()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn w_at_camp_lists_the_phases_and_picking_one_waits_for_it() {
        let mut session = session();
        assert_eq!(session.state().place, game::Place::Camp);
        session.handle(Input::Game(InputEvent::WaitForPhase));
        let Mode::ChoosePhase(chooser) = session.mode() else {
            panic!("the phase list is up");
        };
        assert_eq!(chooser.selected(), 0, "on the phase already chosen");
        let before = session.state().now;
        session.handle(Input::List(OverlayInput::Down));
        session.handle(Input::List(OverlayInput::Select));
        assert!(matches!(session.mode(), Mode::Play));
        assert_eq!(session.state().phase, "Embers");
        assert!(session.state().now > before, "waiting takes a turn");
    }

    #[test]
    fn w_away_from_camp_says_why_not() {
        let blueprints = game::Blueprints::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data"),
        )
        .unwrap()
        .with_camp(None);
        let mut session = Session::new(GameState::new(3, blueprints).unwrap(), "hero");
        session.handle(Input::Game(InputEvent::WaitForPhase));
        assert!(matches!(session.mode(), Mode::Play));
        assert_eq!(last_log(&session), "You can only wait for a phase at camp.");
    }

    #[test]
    fn a_new_game_starts_with_a_bow_a_stocked_quiver_and_a_torch() {
        let state = state();
        assert_eq!(fire_step(&state), FireStep::Aim);
        let worn: Vec<(String, Option<String>)> = game::equipment_view(&state)
            .into_iter()
            .filter_map(|part| part.item)
            .map(|item| (item.name, item.note))
            .collect();
        assert_eq!(
            worn,
            [
                ("quiver".to_string(), Some("12/20".to_string())),
                ("short bow".to_string(), None),
                ("torch".to_string(), None)
            ]
        );
    }

    #[test]
    fn modes_choose_the_key_map() {
        let mut session = session();
        assert_eq!(session.keys(), Keys::Game);
        session.handle(Input::Game(InputEvent::Inventory));
        assert_eq!(session.keys(), Keys::List);
        session.handle(Input::List(OverlayInput::Close));
        session.handle(Input::Game(InputEvent::Fire));
        assert_eq!(session.keys(), Keys::Cursor);
        session.handle(Input::Cursor(AimInput::Cancel));
        assert_eq!(session.keys(), Keys::Game);
    }

    #[test]
    fn moving_takes_a_turn_and_quit_asks_to_leave() {
        let mut session = session();
        let before = session.state().now;
        assert_eq!(
            session.handle(Input::Game(InputEvent::Wait)),
            Outcome::Continue
        );
        assert!(session.state().now > before);
        assert_eq!(session.handle(Input::Game(InputEvent::Quit)), Outcome::Quit);
    }

    #[test]
    fn a_shot_refused_keeps_the_cursor_up() {
        let mut session = session();
        session.handle(Input::Game(InputEvent::Fire));
        // Far out of range: refused, with the reason logged.
        for _ in 0..40 {
            session.handle(Input::Cursor(AimInput::Move(Dir::E)));
        }
        let before = session.state().now;
        session.handle(Input::Cursor(AimInput::Confirm));
        assert_eq!(session.state().now, before);
        assert!(matches!(session.mode(), Mode::Aim(_)));
    }

    #[test]
    fn a_shot_taken_puts_the_cursor_away() {
        let mut session = session();
        session.handle(Input::Game(InputEvent::Fire));
        session.handle(Input::Cursor(AimInput::Move(Dir::E)));
        let before = session.state().now;
        session.handle(Input::Cursor(AimInput::Confirm));
        assert!(session.state().now > before);
        assert!(matches!(session.mode(), Mode::Play));
        assert!(
            session
                .take_fx()
                .iter()
                .any(|fx| matches!(fx, Fx::Flight(_))),
            "the arrow flew"
        );
    }

    #[test]
    fn the_spell_list_opens_and_lists_what_can_be_cast() {
        let mut session = Session::new(crate::tests::state_as("HedgeMage"), "hero");
        session.handle(Input::Game(InputEvent::Cast));
        assert!(matches!(session.mode(), Mode::ChooseSpell(_)));
        session.handle(Input::List(OverlayInput::Select));
        assert!(matches!(session.mode(), Mode::Aim(_)));
        assert!(session.status().contains("Enter: cast"));
    }

    #[test]
    fn the_technique_list_opens_and_aims_what_can_be_performed() {
        let mut session = session();
        // Everyone starts knowing the trip, which needs nothing.
        session.handle(Input::Game(InputEvent::Techniques));
        assert!(matches!(session.mode(), Mode::ChooseTechnique(_)));
        session.handle(Input::List(OverlayInput::Select));
        assert!(matches!(session.mode(), Mode::Aim(_)));
        assert!(session.status().contains("Enter: perform"));
        session.handle(Input::Cursor(AimInput::Cancel));
        assert!(matches!(session.mode(), Mode::Play));
    }

    #[test]
    fn throwing_with_nothing_to_throw_says_so() {
        let mut session = session();
        // Depending on the starting kit: a list, a cursor, or the notice.
        session.handle(Input::Game(InputEvent::Throw));
        match session.mode() {
            Mode::Play => assert_eq!(last_log(&session), "You have nothing to throw."),
            Mode::Choose(_) | Mode::Aim(_) => {}
            other => panic!("unexpected mode {other:?}"),
        }
    }

    #[test]
    fn explore_runs_until_a_key_stops_it() {
        let mut session = session();
        session.handle(Input::Game(InputEvent::Explore));
        assert_eq!(session.auto(), Some(Auto::Explore));
        let before = session.state().now;
        session.auto_step();
        assert!(session.state().now > before);
        // A key stops the run and does nothing else.
        let stopped_at = session.state().now;
        assert_eq!(
            session.handle(Input::Game(InputEvent::Wait)),
            Outcome::Continue
        );
        assert_eq!(session.auto(), None);
        assert_eq!(session.state().now, stopped_at);
    }

    #[test]
    fn enter_while_looking_walks_to_the_looked_at_tile() {
        let mut session = session();
        let start = game::look_targets(session.state())[0];
        session.handle(Input::Game(InputEvent::Look));
        // One tile east, if it is open ground: the generated start may
        // have walls around.
        let to = (start.0 + 1, start.1);
        session.handle(Input::Cursor(AimInput::Set(to)));
        session.handle(Input::Cursor(AimInput::Confirm));
        assert!(matches!(session.mode(), Mode::Play));
        assert_eq!(session.auto(), Some(Auto::Travel(to)));
        while session.auto_step() {}
        let you_are_there = game::look_targets(session.state()).contains(&to);
        let refused = last_log(&session) == "You know no way there.";
        assert!(you_are_there || refused, "{}", last_log(&session));
    }

    #[test]
    fn a_click_to_travel_only_counts_while_playing() {
        let mut session = session();
        session.handle(Input::Game(InputEvent::Inventory));
        session.handle(Input::TravelTo((1, 1)));
        assert_eq!(session.auto(), None);
        session.handle(Input::List(OverlayInput::Close));
        session.handle(Input::TravelTo((1, 1)));
        assert_eq!(session.auto(), Some(Auto::Travel((1, 1))));
    }

    #[test]
    fn the_overlay_takes_turns_and_closes() {
        let mut session = session();
        session.handle(Input::Game(InputEvent::Equipment));
        assert!(matches!(session.mode(), Mode::Overlay(_)));
        session.handle(Input::List(OverlayInput::Close));
        assert!(matches!(session.mode(), Mode::Play));
    }

    #[test]
    fn resting_unhurt_says_so_and_spends_no_time() {
        let mut session = session();
        let before = session.state().now;
        session.handle(Input::Game(InputEvent::Rest));
        assert_eq!(session.auto(), Some(Auto::Rest));
        assert!(!session.auto_step(), "nothing to heal");
        assert_eq!(session.auto(), None);
        assert_eq!(session.state().now, before);
        assert_eq!(last_log(&session), "You are rested.");
    }

    #[test]
    fn on_a_floor_the_status_shows_the_rest_budget() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let blueprints = game::Blueprints::load(&data).unwrap().with_camp(None);
        let state = GameState::new(3, blueprints).unwrap();
        // 125% of the player's 20 health.
        assert!(
            Session::new(state, "hero")
                .status()
                .contains("  Rest: 25  Turn: 0")
        );
    }

    #[test]
    fn status_shows_the_place_and_turn_and_announces_game_over() {
        let mut state = state();
        state.now = Tick(250);
        assert_eq!(
            Session::new(state, "hero").status(),
            "Camp  Turn: 2  Seed: 3"
        );
        let mut state = crate::tests::state();
        state.now = Tick(700);
        // What the frontend reads of a death; how dying works is `game`'s.
        state.game_over = true;
        let mut session = Session::new(state, "hero");
        assert_eq!(
            session.status(),
            "You died on turn 7. Press any key to return to the title."
        );
        assert_eq!(session.keys(), Keys::Any);
        assert_eq!(session.handle(Input::Any), Outcome::Quit);
    }
}
