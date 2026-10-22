//! Test helpers: a game set up from the real data, and the app in a headless window (`egui_kittest`), fed key and pointer events like eframe does.

use std::cell::Cell;
use std::path::Path;
use std::time::Instant;

use eframe::egui::{self, Event};
use egui_kittest::Harness;
use front::title::Setup;
use game::Tick;

use crate::app::App;
use crate::input::presses;

thread_local! {
    /// The time the window's frames see, when a test sets one.
    static CLOCK: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Freezes the window's clock at `at` (or lets it run with `None`), so a
/// test can render an animation at chosen moments. Per test thread.
pub fn set_clock(at: Option<Instant>) {
    CLOCK.with(|clock| clock.set(at));
}

fn clock() -> Instant {
    CLOCK.with(Cell::get).unwrap_or_else(Instant::now)
}

/// The test window's size: `main`'s default window.
pub const WINDOW: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// A fresh saves directory per test, under the system temp dir.
pub fn setup(name: &str) -> Setup {
    let saves_dir =
        std::env::temp_dir().join(format!("roguelike-gui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&saves_dir);
    Setup {
        data_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("../data"),
        saves_dir,
        seed: Some(3),
    }
}

pub fn now(app: &App) -> Tick {
    app.game().unwrap().session.state().now
}

/// The app in a headless window, fed key and pointer events like eframe
/// does. Fonts added in one pass apply from the next, as `main` adds
/// them before the first. The harness panics on a glyph no font has, so
/// every test through it also checks what it drew can be drawn.
pub fn window(name: &str) -> Harness<'static, App> {
    harness(App::new(setup(name), Some("hero".into())))
}

pub fn harness(app: App) -> Harness<'static, App> {
    let mut fonts_installed = false;
    let mut harness = Harness::builder().with_size(WINDOW).build_ui_state(
        move |ui, app: &mut App| {
            if !fonts_installed {
                crate::theme::apply(ui.ctx());
                fonts_installed = true;
                ui.ctx().request_repaint();
                return;
            }
            let events = ui.input(|input| input.events.clone());
            let now = clock();
            app.update(&presses(&events), now);
            app.show(ui, now);
        },
        app,
    );
    harness.run();
    harness
}

pub fn type_text(harness: &mut Harness<'static, App>, text: &str) {
    harness.event(Event::Text(text.into()));
    harness.run();
}

/// A named key pressed and handled, as eframe sends it.
pub fn press_key(harness: &mut Harness<'static, App>, key: egui::Key) {
    harness.event(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
}

pub fn mode(harness: &Harness<'static, App>) -> String {
    format!("{:?}", harness.state().game().unwrap().session.mode())
}
