//! Renders each screen to PNGs for a human or an agent to look at, since nobody may run the window from here (`scripts/shots.sh`).

use std::time::Instant;

use eframe::egui::{self, Event};

use crate::testing::{harness, setup, type_text, window};

use super::*;

/// Renders each screen to PNGs for a human to look at, since nobody may
/// run the window from here. Needs a GPU, so it only runs when asked:
/// `GUI_SCREENSHOTS=<dir> cargo test -p gui screenshots -- --ignored`.
#[test]
#[ignore = "needs a GPU; writes PNGs to GUI_SCREENSHOTS"]
fn screenshots() {
    let dir = std::path::PathBuf::from(std::env::var("GUI_SCREENSHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let mut menu = harness(App::new(setup("screenshots-menu"), None));
    let image = menu.render().unwrap();
    image.save(dir.join("menu.png")).unwrap();
    let mut harness = window("screenshots");
    for (keys, name) in [
        ("", "play"),
        ("i", "inventory"),
        ("q;", "look"),
        ("qc", "spells"),
        ("qf", "aim"),
    ] {
        for key in keys.chars() {
            type_text(&mut harness, &key.to_string());
        }
        let image = harness.render().unwrap();
        image.save(dir.join(format!("{name}.png"))).unwrap();
    }
    // Effects around the player, caught early in their rise.
    type_text(&mut harness, "q;");
    let front::session::Mode::Look(looking) = harness.state().game().unwrap().session.mode() else {
        panic!("looking");
    };
    let (x, y) = looking.cursor();
    type_text(&mut harness, "q");
    let fx = vec![
        game::Fx::Damage {
            at: (x, y),
            amount: 3,
            blocked: 0,
            to_player: true,
        },
        game::Fx::Outcome {
            at: (x + 3, y),
            kind: game::Outcome::Deflect,
        },
        game::Fx::Death { at: (x - 2, y) },
        game::Fx::Flight(game::Trail {
            glyph: '|',
            fg: game::Rgb(200, 160, 90),
            path: (1..8).map(|dx| (x, y - dx)).collect(),
        }),
    ];
    harness.state_mut().effects.add(fx, Instant::now());
    harness.step();
    let image = harness.render().unwrap();
    image.save(dir.join("effects.png")).unwrap();
    harness.event(Event::Key {
        key: egui::Key::F2,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    assert!(harness.state().atlas.is_some(), "the atlas loaded");
    let image = harness.render().unwrap();
    image.save(dir.join("sprites.png")).unwrap();
}
