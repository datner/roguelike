use std::time::Instant;

use eframe::egui::{self, Event};

use super::*;
use crate::testing::{mode, now, setup, type_text, window};

#[test]
fn a_named_character_starts_straight_away_and_moves() {
    let mut app = App::new(setup("moves"), Some("hero".into()));
    let before = now(&app);
    app.update(&[Press::Char('.')], Instant::now());
    assert!(now(&app) > before);
}

#[test]
fn quitting_saves_and_returns_to_the_menu() {
    let setup = setup("quits");
    let saves = setup.saves_dir.clone();
    let mut app = App::new(setup, Some("hero".into()));
    app.update(&[Press::Char('q')], Instant::now());
    assert!(app.game().is_none());
    assert_eq!(front::saves::list(&saves).unwrap(), ["hero"]);
    // Esc on the menu closes the window.
    assert_eq!(app.update(&[Press::Esc], Instant::now()), Flow::Exit);
}

#[test]
fn auto_explore_steps_on_its_clock_and_any_key_stops_it() {
    let mut app = App::new(setup("explores"), Some("hero".into()));
    let start = Instant::now();
    app.update(&[Press::Char('x')], start);
    let before = now(&app);
    app.update(&[], start);
    assert_eq!(now(&app), before, "not due yet");
    app.update(&[], start + AUTO_STEP_DELAY);
    assert!(now(&app) > before, "a step was taken");
    let stepped = now(&app);
    app.update(&[Press::Char('z')], start + AUTO_STEP_DELAY * 2);
    assert_eq!(now(&app), stepped, "an unmapped key only stops the run");
    assert!(app.game().unwrap().session.auto().is_none());
}

#[test]
fn a_shot_plays_its_flight_until_a_key_finishes_it() {
    let mut app = App::new(setup("fx"), Some("hero".into()));
    let now = Instant::now();
    let aim_and_shoot = [
        Press::Char('f'),
        Press::Right,
        Press::Right,
        Press::Right,
        Press::Enter,
    ];
    app.update(&aim_and_shoot, now);
    assert!(app.effects.update(now), "the arrow is in flight");
    app.update(&[Press::Char('z')], now);
    assert!(!app.effects.update(now), "a key finished it");
}

#[test]
fn f2_switches_sprites_without_taking_a_turn() {
    let mut app = App::new(setup("sprites"), Some("hero".into()));
    let before = now(&app);
    app.update(&[Press::F2], Instant::now());
    assert!(app.sprites);
    assert_eq!(now(&app), before);
    app.update(&[Press::F2], Instant::now());
    assert!(!app.sprites);
}

#[test]
fn the_window_plays_from_typed_keys() {
    let mut harness = window("window");
    let before = now(harness.state());
    type_text(&mut harness, ".");
    assert!(now(harness.state()) > before);
}

#[test]
fn every_mode_draws_over_the_map() {
    let mut harness = window("modes");
    for (open, shown) in [
        ("i", "Overlay"),
        ("q", "Play"),
        ("e", "Overlay"),
        ("q", "Play"),
        ("S", "Overlay"),
        ("q", "Play"),
        (";", "Look"),
        ("q", "Play"),
        ("a", "ChooseTechnique"),
        ("q", "Play"),
        ("f", "Aim"),
    ] {
        type_text(&mut harness, open);
        assert!(
            mode(&harness).starts_with(shown),
            "{open}: {}",
            mode(&harness)
        );
    }
}

#[test]
fn a_hedge_mage_opens_the_spell_list() {
    let mut harness = crate::testing::window_as("spells", "HedgeMage");
    type_text(&mut harness, "c");
    assert!(
        mode(&harness).starts_with("ChooseSpell"),
        "{}",
        mode(&harness)
    );
}

#[test]
fn hovering_the_map_moves_the_aim_cursor_and_a_click_shoots() {
    let mut harness = window("pointer");
    type_text(&mut harness, "f");
    let before = now(harness.state());
    // The map fills the window left of the side column, above the log;
    // a point a few tiles right of its middle is on the map.
    let map_middle = egui::pos2((1280.0 - SIDE_WIDTH) / 2.0, 300.0);
    harness.hover_at(map_middle + egui::vec2(3.0 * crate::map_view::TILE.x, 0.0));
    harness.run();
    assert!(mode(&harness).starts_with("Aim"));
    harness.event(Event::PointerButton {
        pos: map_middle + egui::vec2(3.0 * crate::map_view::TILE.x, 0.0),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.event(Event::PointerButton {
        pos: map_middle + egui::vec2(3.0 * crate::map_view::TILE.x, 0.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    assert!(now(harness.state()) > before, "the click fired");
    assert!(mode(&harness).starts_with("Play"));
}

#[test]
fn time_played_adds_up_frames_and_caps_an_idle_gap() {
    let setup = setup("plays");
    let saves = setup.saves_dir.clone();
    let mut app = App::new(setup, Some("hero".into()));
    let start = Instant::now();
    app.update(&[], start);
    app.update(&[], start + std::time::Duration::from_secs(10));
    // Away from the window for an hour: counts as `IDLE`.
    app.update(&[], start + std::time::Duration::from_secs(3610));
    assert_eq!(app.played, std::time::Duration::from_secs(10) + IDLE);
    app.update(
        &[Press::Char('q')],
        start + std::time::Duration::from_secs(3610),
    );
    let text = std::fs::read_to_string(saves.join("playtime.csv")).unwrap();
    assert!(text.ends_with("hero,0,40\n"), "{text}");
}
