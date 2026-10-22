//! Renders screens to PNGs for a human or an agent to look at, since nobody may run the window from here (`scripts/shots.sh`). Each shot is set up on its own, so any subset can be rendered, and cropped to the part of the window that changed: an image costs about width × height / 750 tokens to read.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use image::RgbaImage;

use crate::panels::{BOTTOM_HEIGHT, SIDE_WIDTH};
use crate::testing::{WINDOW, harness, press_key, set_clock, setup, type_text, window};

use super::*;

/// Every shot, in the order they render.
const SHOTS: [&str; 12] = [
    "menu",
    "play",
    "inventory",
    "look",
    "spells",
    "aim",
    "effects",
    "sprites",
    "fx",
    "phases",
    "floor",
    "castle",
];

/// The `fx` sheet: this many frames, this far apart, from when the effects
/// start.
const FRAMES: u32 = 9;
const FRAME: Duration = Duration::from_millis(100);
/// Each frame: the map around the player, in pixels.
const AROUND: (f32, f32) = (300.0, 160.0);

/// The root `Ui`'s margin in the test window, around everything drawn.
const MARGIN: f32 = 8.0;

/// A part of the window, from the layout's own sizes.
fn region(name: &str) -> Option<(f32, f32, f32, f32)> {
    let (width, height) = (WINDOW.x, WINDOW.y);
    let side = width - SIDE_WIDTH - MARGIN;
    let bottom = height - BOTTOM_HEIGHT - MARGIN;
    match name {
        "all" => Some((0.0, 0.0, width, height)),
        "map" => Some((0.0, 0.0, side, bottom)),
        "side" => Some((side, 0.0, width - side, height)),
        "bottom" => Some((0.0, bottom, side, height - bottom)),
        _ => None,
    }
}

fn crop(image: &RgbaImage, (x, y, w, h): (f32, f32, f32, f32)) -> RgbaImage {
    image::imageops::crop_imm(image, x as u32, y as u32, w as u32, h as u32).to_image()
}

/// The world tile the player stands on: where the look cursor starts.
fn player_tile(harness: &mut Harness<'static, App>) -> (i32, i32) {
    type_text(harness, ";");
    let front::session::Mode::Look(looking) = harness.state().game().unwrap().session.mode() else {
        panic!("looking");
    };
    let at = looking.cursor();
    type_text(harness, "q");
    at
}

/// Effects around `(x, y)`: the player hurt, a deflected blow, a death and
/// an arrow flying off north.
fn effects_around((x, y): (i32, i32)) -> Vec<game::Fx> {
    vec![
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
            path: (1..8).map(|dy| (x, y - dy)).collect(),
        }),
    ]
}

/// The window showing `name`, from a fresh game (or the title menu), on a
/// stopped clock: animation shows its first moment, so a shot is the same
/// every run and can be compared with its reference.
fn scene(name: &str) -> Harness<'static, App> {
    let start = Instant::now();
    set_clock(Some(start));
    if name == "menu" {
        return harness(App::new(setup("shot-menu"), None));
    }
    let mut harness = window(&format!("shot-{name}"));
    harness.state_mut().started = start;
    match name {
        "inventory" => type_text(&mut harness, "i"),
        "look" => type_text(&mut harness, ";"),
        "spells" => type_text(&mut harness, "c"),
        "aim" => type_text(&mut harness, "f"),
        "phases" => type_text(&mut harness, "W"),
        // Round the tents to the camp's entrance (data/camp.ron), then down.
        "floor" => type_text(&mut harness, &format!("kkk{}jjj>", "l".repeat(14))),
        // The same round the tents, west to the castle's entrance.
        "castle" => type_text(&mut harness, &format!("kkk{}jjj>", "h".repeat(12))),
        "effects" => {
            // A tenth of the way into their rise.
            let fx = effects_around(player_tile(&mut harness));
            harness.state_mut().effects.add(fx, start);
            set_clock(Some(start + FRAME));
            harness.step();
        }
        "sprites" => {
            press_key(&mut harness, egui::Key::F2);
            assert!(harness.state().atlas.is_some(), "the atlas loaded");
        }
        _ => {}
    }
    harness
}

/// The effects around the player every `FRAME`, left to right with a thin
/// gap: one image shows how they move, rise and fade.
fn fx_sheet() -> RgbaImage {
    let mut harness = window("shot-fx");
    let at = player_tile(&mut harness);
    let start = Instant::now();
    set_clock(Some(start));
    harness.state_mut().effects.add(effects_around(at), start);
    let (x, y, w, h) = region("map").unwrap();
    let (cw, ch) = AROUND;
    let around = (x + (w - cw) / 2.0, y + (h - ch) / 2.0, cw, ch);
    let frames: Vec<RgbaImage> = (0..FRAMES)
        .map(|k| {
            set_clock(Some(start + FRAME * k));
            harness.step();
            crop(&harness.render().unwrap(), around)
        })
        .collect();
    set_clock(None);
    side_by_side(&frames)
}

fn side_by_side(frames: &[RgbaImage]) -> RgbaImage {
    const GAP: u32 = 2;
    let (w, h) = frames.first().map_or((0, 0), RgbaImage::dimensions);
    let count = u32::try_from(frames.len()).unwrap();
    let mut sheet = RgbaImage::from_pixel(
        count * w + count.saturating_sub(1) * GAP,
        h,
        image::Rgba([128, 128, 128, 255]),
    );
    for (k, frame) in (0..).zip(frames) {
        image::imageops::replace(&mut sheet, frame, i64::from(k * (w + GAP)), 0);
    }
    sheet
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(std::env::var("GUI_SCREENSHOTS").expect("GUI_SCREENSHOTS=<dir>"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `GUI_SHOTS` (comma-separated), or every shot.
fn chosen() -> Vec<&'static str> {
    match std::env::var("GUI_SHOTS") {
        Ok(names) if !names.is_empty() => names
            .split(',')
            .map(|name| {
                *SHOTS
                    .iter()
                    .find(|shot| **shot == name)
                    .unwrap_or_else(|| panic!("no shot {name:?}; shots: {SHOTS:?}"))
            })
            .collect(),
        _ => SHOTS.to_vec(),
    }
}

fn save(dir: &Path, name: &str, image: &RgbaImage) {
    image.save(dir.join(format!("{name}.png"))).unwrap();
}

/// Renders the shots `GUI_SHOTS` names (all by default) into
/// `GUI_SCREENSHOTS`, cropped to `GUI_CROP` (`map`, `side`, `bottom`; the
/// whole window by default). `fx` is a strip of frames of the effects
/// animating, cropped around the player.
/// With `GUI_CHECK=1` each is also compared with its reference in
/// `gui/tests/snapshots/` (whole window; `fx` as is). Needs a GPU, so it only runs when asked.
#[test]
#[ignore = "needs a GPU; run through scripts/shots.sh"]
fn screenshots() {
    let dir = out_dir();
    let crop_to = std::env::var("GUI_CROP").unwrap_or_else(|_| "all".into());
    let region = region(&crop_to).unwrap_or_else(|| panic!("no region {crop_to:?}"));
    let check = std::env::var("GUI_CHECK").is_ok_and(|value| value == "1");
    let mut failed = Vec::new();
    for name in chosen() {
        let image = match name {
            "fx" => fx_sheet(),
            _ => crop(&scene(name).render().unwrap(), region),
        };
        save(&dir, name, &image);
        if check && let Err(err) = compare(name, &image) {
            failed.push(format!("{name}: {err}"));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Where the reference shots live, beside their diffs on a mismatch.
fn references() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

/// `image` against its reference (`UPDATE_SNAPSHOTS=1` rewrites a
/// mismatching one). On a mismatch `<name>.diff.png` shows what changed.
fn compare(name: &str, image: &RgbaImage) -> Result<(), String> {
    let options = egui_kittest::SnapshotOptions::new().output_path(references());
    egui_kittest::try_image_snapshot_options(image, name, &options).map_err(|err| err.to_string())
}
