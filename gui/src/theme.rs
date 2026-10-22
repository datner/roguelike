//! The terminal look: one monospace font on a grid of cells the size of a map tile, black background, titled boxes with thin borders, the selected row in reverse video. Everything `gui` draws goes through these, so the window reads like `term`.

use eframe::egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontFamily, FontId, Frame, Margin, Rect, Shadow, Shape,
    Stroke, TextFormat, TextStyle, Ui, UiBuilder, text::LayoutJob, vec2,
};

use crate::map_view::TILE;

/// Text size in points: a glyph of DejaVu Sans Mono fills one `TILE`.
pub const TEXT: f32 = 16.0;
/// A terminal's default foreground.
pub const FG: Color32 = Color32::from_gray(204);
/// `Modifier::BOLD` in most terminals: brighter.
pub const BRIGHT: Color32 = Color32::WHITE;
/// `Modifier::DIM`.
pub const DIM: Color32 = Color32::from_gray(120);
pub const BORDER: Color32 = Color32::from_gray(150);
pub const BG: Color32 = Color32::BLACK;

/// DejaVu Sans Mono for all text, as a terminal has one font; egui's own
/// fonts stay behind it. It also has what egui's lack (`☺` on the map,
/// `↑↓` in hints). Licence beside the file in `gui/assets`.
pub fn install_fonts(ctx: &egui::Context) {
    let first = |family| InsertFontFamily {
        family,
        priority: FontPriority::Highest,
    };
    ctx.add_font(FontInsert::new(
        "DejaVu Sans Mono",
        FontData::from_static(include_bytes!("../assets/DejaVuSansMono.ttf")),
        vec![
            first(FontFamily::Monospace),
            first(FontFamily::Proportional),
        ],
    ));
}

/// Fonts and the terminal style, for the light and dark themes alike.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.all_styles_mut(|style| {
        for (text_style, font) in &mut style.text_styles {
            *font = FontId::monospace(match text_style {
                TextStyle::Small => TEXT - 2.0,
                _ => TEXT,
            });
        }
        let spacing = &mut style.spacing;
        spacing.item_spacing = vec2(0.0, 0.0);
        spacing.button_padding = vec2(0.0, 0.0);
        spacing.interact_size.y = TILE.y;
        spacing.window_margin = Margin::ZERO;
        spacing.menu_margin = Margin::symmetric(TILE.x as i8, (TILE.y / 2.0) as i8);

        let visuals = &mut style.visuals;
        visuals.dark_mode = true;
        visuals.override_text_color = None;
        visuals.weak_text_color = Some(DIM);
        visuals.panel_fill = BG;
        visuals.window_fill = BG;
        visuals.extreme_bg_color = BG;
        visuals.faint_bg_color = BG;
        visuals.window_stroke = Stroke::new(1.0, BORDER);
        visuals.window_corner_radius = CornerRadius::ZERO;
        visuals.menu_corner_radius = CornerRadius::ZERO;
        visuals.window_shadow = Shadow::NONE;
        visuals.popup_shadow = Shadow::NONE;
        visuals.selection.bg_fill = FG;
        visuals.selection.stroke = Stroke::new(1.0, BG);
        let widgets = &mut visuals.widgets;
        for (state, fg, bg) in [
            (&mut widgets.noninteractive, FG, BG),
            (&mut widgets.inactive, FG, BG),
            (&mut widgets.hovered, BRIGHT, Color32::from_gray(50)),
            (&mut widgets.active, BRIGHT, Color32::from_gray(70)),
            (&mut widgets.open, BRIGHT, Color32::from_gray(50)),
        ] {
            state.fg_stroke = Stroke::new(1.0, fg);
            state.bg_fill = bg;
            state.weak_bg_fill = bg;
            state.bg_stroke = Stroke::NONE;
            state.corner_radius = CornerRadius::ZERO;
            state.expansion = 0.0;
        }
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    });
}

/// One line of text in `colour`, a row of cells high.
pub fn line(text: &str, colour: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    append(&mut job, text, colour);
    job
}

/// Adds text to a line, keeping it a row of cells high.
pub fn append(job: &mut LayoutJob, text: &str, colour: Color32) {
    let format = TextFormat {
        line_height: Some(TILE.y),
        ..TextFormat::simple(FontId::monospace(TEXT), colour)
    };
    job.append(text, 0.0, format);
}

/// A box with its title on the top border and, if any, a dim hint on the
/// bottom one: ratatui's `Block::bordered().title(..).title_bottom(..)`.
/// `title` lays out its own widgets (plain text, or clickable tabs); the
/// border is cleared behind them.
pub fn boxed<R>(
    ui: &mut Ui,
    title: impl FnOnce(&mut Ui),
    hint: Option<&str>,
    contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let half = TILE.y / 2.0;
    let inner = Frame::new()
        .stroke(Stroke::new(1.0, BORDER))
        .inner_margin(Margin::symmetric(TILE.x as i8, half as i8))
        .outer_margin(Margin {
            top: half as i8,
            bottom: half as i8,
            ..Margin::ZERO
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            contents(ui)
        });
    let border = inner.response.rect.shrink2(vec2(0.0, half));
    let width = border.width() - 2.0 * TILE.x;
    label_on(ui, border.left_top(), width, title);
    if let Some(hint) = hint {
        label_on(ui, border.left_bottom(), width, |ui| {
            ui.label(line(hint, DIM));
        });
    }
    inner.inner
}

/// A plain title for `boxed`.
pub fn title(text: &str) -> impl FnOnce(&mut Ui) + '_ {
    move |ui: &mut Ui| {
        ui.label(line(text, FG));
    }
}

/// Lays out `contents` a cell in from `corner`, centred on the border line,
/// over a background that hides the line.
fn label_on(ui: &mut Ui, corner: egui::Pos2, width: f32, contents: impl FnOnce(&mut Ui)) {
    let clear = ui.painter().add(Shape::Noop);
    let rect = Rect::from_min_size(
        corner + vec2(TILE.x, -TILE.y / 2.0),
        vec2(width.max(0.0), TILE.y),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    contents(&mut child);
    ui.painter()
        .set(clear, Shape::rect_filled(child.min_rect(), 0.0, BG));
}

/// One line in reverse video, as `term` marks the selected row.
pub fn reversed(text: &str) -> LayoutJob {
    let mut job = line(text, BG);
    for section in &mut job.sections {
        section.format.background = FG;
    }
    job
}
