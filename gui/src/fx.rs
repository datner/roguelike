//! Effects on the map (ADR 0022), on the frame clock: a missile glides from tile to tile, damage and what a blow did rise from the struck tile, a struck tile flashes in reverse video and a dying one red, and the map shakes when the player is hurt. Glyphs and text on the cell grid, so they keep `term`'s look. Any input finishes them all, as it cuts `term`'s flights short.

use std::time::{Duration, Instant};

use eframe::egui::{Align2, Color32, FontId, Painter, Rect, Vec2, vec2};
use game::{Cell, Fx, GameState, Grid, Outcome, Rgb};

use crate::map_view::{Layout, TILE, color, screen_tile};
use crate::theme::{BG, FG, TEXT};

/// Time a missile spends on each tile it crosses, as in `term`.
pub const STEP: Duration = Duration::from_millis(40);
/// How long damage and outcome words take to rise and fade.
pub const RISE: Duration = Duration::from_millis(800);
/// The start of a hit's rise during which the tile flashes.
const FLASH: f32 = 0.2;
pub const DEATH: Duration = Duration::from_millis(400);
/// The start of the player's damage during which the map shakes.
const SHAKE: f32 = 0.3;

/// A world tile.
type Tile = (i32, i32);

const HURT: Rgb = Rgb(230, 70, 70);
const DAMAGE: Rgb = Rgb(240, 200, 90);
const DYING: Rgb = Rgb(150, 20, 20);

struct Playing {
    fx: Fx,
    start: Instant,
}

/// The effects playing, each from when it starts.
#[derive(Default)]
pub struct Effects {
    playing: Vec<Playing>,
}

pub fn duration(fx: &Fx) -> Duration {
    match fx {
        Fx::Flight(trail) => STEP * u32::try_from(trail.path.len()).unwrap_or(u32::MAX),
        Fx::Damage { .. } | Fx::Outcome { .. } => RISE,
        Fx::Death { .. } => DEATH,
    }
}

/// How far along something started at `start` is at `now`, from 0 to 1;
/// `None` before it starts and once it is over.
pub fn progress(start: Instant, duration: Duration, now: Instant) -> Option<f32> {
    let elapsed = now.checked_duration_since(start)?;
    (elapsed < duration).then(|| elapsed.as_secs_f32() / duration.as_secs_f32())
}

/// Where a missile `t` of the way along `path` is: between two tiles, and how
/// far from the first. It glides only between neighbouring tiles; an area,
/// listed nearest the centre first, appears tile by tile.
pub fn flight_at(path: &[Tile], t: f32) -> Option<(Tile, Tile, f32)> {
    let last = path.len().checked_sub(1)?;
    let along = (t.clamp(0.0, 1.0) * path.len() as f32).min(last as f32);
    let index = along.floor() as usize;
    let from = path[index];
    let to = path[(index + 1).min(last)];
    let neighbours = (from.0 - to.0).abs() <= 1 && (from.1 - to.1).abs() <= 1;
    Some(if neighbours {
        (from, to, along.fract())
    } else {
        (from, from, 0.0)
    })
}

impl Effects {
    /// Queues new effects. Flights play one after another, as they flew;
    /// what came after a flight (its hit) starts when it lands.
    pub fn add(&mut self, fx: Vec<Fx>, now: Instant) {
        let mut start = self
            .playing
            .iter()
            .filter(|playing| matches!(playing.fx, Fx::Flight(_)))
            .map(|playing| playing.start + duration(&playing.fx))
            .max()
            .map_or(now, |end| end.max(now));
        for fx in fx {
            let begins = start;
            if let Fx::Flight(_) = fx {
                start += duration(&fx);
            }
            self.playing.push(Playing { fx, start: begins });
        }
    }

    pub fn finish(&mut self) {
        self.playing.clear();
    }

    /// Drops what has ended; whether anything is left to play.
    pub fn update(&mut self, now: Instant) -> bool {
        self.playing
            .retain(|playing| now < playing.start + duration(&playing.fx));
        !self.playing.is_empty()
    }

    /// What is showing at `now`, and how far along.
    fn showing(&self, now: Instant) -> impl Iterator<Item = (&Fx, f32)> {
        self.playing.iter().filter_map(move |playing| {
            progress(playing.start, duration(&playing.fx), now).map(|t| (&playing.fx, t))
        })
    }

    /// How far the map is pushed this frame: a short, fading jolt while the
    /// player is hurt.
    pub fn shake(&self, now: Instant) -> Vec2 {
        self.showing(now)
            .filter_map(|(fx, t)| match fx {
                Fx::Damage {
                    to_player: true, ..
                } if t < SHAKE => Some(t / SHAKE),
                _ => None,
            })
            .map(|q| {
                let fade = 1.0 - q;
                vec2(3.0 * fade * (q * 40.0).sin(), 2.0 * fade * (q * 53.0).cos())
            })
            .next()
            .unwrap_or(Vec2::ZERO)
    }

    /// Flashes on the map's cells, before they are drawn: a struck tile in
    /// reverse video, a dying one red.
    pub fn mark(&self, grid: &mut Grid<Cell>, state: &GameState, layout: &Layout, now: Instant) {
        for (fx, t) in self.showing(now) {
            let (at, flash) = match *fx {
                Fx::Damage { at, .. } if t < FLASH => (at, None),
                Fx::Death { at } => (at, Some(DYING)),
                _ => continue,
            };
            let Some((x, y)) = screen_tile(state, layout, at) else {
                continue;
            };
            let Some(cell) = grid.get_mut(x, y) else {
                continue;
            };
            match flash {
                Some(bg) => cell.bg = bg,
                None => {
                    cell.bg = cell.fg;
                    cell.fg = Rgb(0, 0, 0);
                }
            }
        }
    }

    /// Missiles and rising words, over the drawn map.
    pub fn draw(&self, painter: &Painter, state: &GameState, layout: &Layout, now: Instant) {
        let font = FontId::monospace(TEXT);
        let rect_of = |at| screen_tile(state, layout, at).map(|tile| layout.tile_rect(tile));
        for (fx, t) in self.showing(now) {
            match fx {
                Fx::Flight(trail) => {
                    let Some((from, to, frac)) = flight_at(&trail.path, t) else {
                        continue;
                    };
                    let (Some(from), Some(to)) = (rect_of(from), rect_of(to)) else {
                        continue;
                    };
                    let rect = Rect::from_min_size(from.min.lerp(to.min, frac), TILE);
                    painter.rect_filled(rect, 0.0, BG);
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        trail.glyph,
                        font.clone(),
                        color(trail.fg),
                    );
                }
                &Fx::Damage {
                    at,
                    amount,
                    to_player,
                    ..
                } => {
                    let colour = if to_player { HURT } else { DAMAGE };
                    rise(
                        painter,
                        rect_of(at),
                        0.5,
                        &amount.to_string(),
                        color(colour),
                        t,
                    );
                }
                &Fx::Outcome { at, kind } => {
                    // A row above the damage, which a graze also deals.
                    rise(painter, rect_of(at), 1.5, word(kind), FG, t);
                }
                Fx::Death { .. } => {}
            }
        }
    }
}

fn word(kind: Outcome) -> &'static str {
    match kind {
        Outcome::Miss => "miss",
        Outcome::Graze => "graze",
        Outcome::Dodge => "dodge",
        Outcome::Deflect => "deflect",
        Outcome::Absorbed => "absorbed",
    }
}

/// Text rising a row from `lift` rows above `tile` and fading, on a black
/// backing so it reads over the map.
fn rise(painter: &Painter, tile: Option<Rect>, lift: f32, text: &str, colour: Color32, t: f32) {
    let Some(tile) = tile else {
        return;
    };
    let fade = 1.0 - t * t;
    let centre = tile.center() - vec2(0.0, TILE.y * (lift + t));
    let galley = painter.layout_no_wrap(text.into(), FontId::monospace(TEXT), colour);
    let rect = Align2::CENTER_CENTER.anchor_size(centre, galley.size());
    painter.rect_filled(rect, 0.0, BG.gamma_multiply(fade));
    painter.galley(rect.min, galley, colour.gamma_multiply(fade));
}

#[cfg(test)]
mod tests {
    use game::Trail;

    use super::*;

    fn flight(len: i32) -> Fx {
        Fx::Flight(Trail {
            glyph: '|',
            fg: Rgb(1, 1, 1),
            path: (1..=len).map(|x| (x, 0)).collect(),
        })
    }

    #[test]
    fn progress_runs_from_start_to_end() {
        let start = Instant::now();
        let second = Duration::from_secs(1);
        assert_eq!(progress(start + second, second, start), None, "not yet");
        assert_eq!(progress(start, second, start), Some(0.0));
        assert_eq!(progress(start, second, start + second / 2), Some(0.5));
        assert_eq!(progress(start, second, start + second), None, "over");
    }

    #[test]
    fn a_missile_glides_between_neighbours_and_jumps_across_an_area() {
        let line = [(1, 0), (2, 0), (3, 0), (4, 0)];
        assert_eq!(flight_at(&line, 0.0), Some(((1, 0), (2, 0), 0.0)));
        assert_eq!(flight_at(&line, 0.375), Some(((2, 0), (3, 0), 0.5)));
        assert_eq!(flight_at(&line, 1.0), Some(((4, 0), (4, 0), 0.0)));
        let area = [(5, 5), (7, 5)];
        assert_eq!(flight_at(&area, 0.25), Some(((5, 5), (5, 5), 0.0)));
        assert_eq!(flight_at(&[], 0.5), None);
    }

    #[test]
    fn flights_play_in_turn_and_their_hits_wait_for_them() {
        let now = Instant::now();
        let mut effects = Effects::default();
        let death = Fx::Death { at: (3, 0) };
        effects.add(vec![flight(3), flight(2), death], now);
        let starts: Vec<Instant> = effects.playing.iter().map(|p| p.start).collect();
        assert_eq!(starts, [now, now + STEP * 3, now + STEP * 5]);

        // Effects added mid-flight wait for it too.
        effects.add(vec![Fx::Death { at: (0, 0) }], now + STEP);
        assert_eq!(effects.playing[3].start, now + STEP * 5);

        assert!(effects.update(now + STEP * 5));
        assert!(!effects.update(now + STEP * 5 + DEATH));
    }

    #[test]
    fn only_the_players_hurt_shakes_the_map() {
        let now = Instant::now();
        let hurt = |to_player| Fx::Damage {
            at: (0, 0),
            amount: 2,
            blocked: 0,
            to_player,
        };
        let mut effects = Effects::default();
        effects.add(vec![hurt(false)], now);
        assert_eq!(effects.shake(now + RISE / 10), Vec2::ZERO);
        effects.add(vec![hurt(true)], now);
        assert_ne!(effects.shake(now + RISE / 10), Vec2::ZERO);
        assert_eq!(effects.shake(now + RISE / 2), Vec2::ZERO, "settled");
        effects.finish();
        assert!(!effects.update(now));
    }
}
