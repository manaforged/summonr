use super::input::{glass, overlay};
use super::logo::{COMPOSER_ROWS, Composed, FACT_LABEL, FADE_MS, layout};
use super::space::Plate;
use super::sprite::{FRAME_MS, Pixel, Size};
use super::*;
use sigil::animation::Easing;
use sigil::geometry::{Point, Rect};
use sigil::scene::{DrawRect, SceneBuilder};
use summonr_agent::prefs::{Splash, Tint};

const GAP: u16 = 4;
const ENTRANCE_MS: u64 = 600;
const MOTE_STEP: u64 = 2;
const MOTE_PHASE: u64 = 3;

/// Whether `splash` draws the pixel Will lockup rather than the classic mark.
pub(crate) const fn drawn(splash: Splash) -> bool {
    !matches!(splash, Splash::Off | Splash::Classic)
}

/// Milliseconds until the lockup has finished its entrance.
pub(crate) const fn reveal_ms() -> u64 {
    ENTRANCE_MS + FADE_MS
}

struct Plan {
    size: Size,
    will: [u16; 2],
    facts: [u16; 2],
    card: [u16; 4],
    value_width: usize,
}

fn plan(cols: u16, rows: u16, facts: &[(&str, String)]) -> Plan {
    let longest = facts
        .iter()
        .map(|(_, value)| value.chars().count() as u16)
        .max()
        .unwrap_or(20);
    let fit = |size: Size| {
        let [width, height] = size.cells();
        let room = cols.saturating_sub(width + GAP + FACT_LABEL + 6);
        let facts_width = FACT_LABEL + longest.min(room);
        let body = [width + GAP + facts_width, height.max(facts.len() as u16)];
        let fits = body[1] + 2 + COMPOSER_ROWS <= rows && body[0] + 6 <= cols;
        (fits, body, longest.min(room))
    };
    let size = [Size::Banner, Size::Compact]
        .into_iter()
        .find(|size| fit(*size).0)
        .unwrap_or(Size::Compact);
    let (_, [body_width, body_rows], value_width) = fit(size);
    let [body_x, body_y, card_x, card_y, card_width] = layout(cols, rows, body_width, body_rows);
    let [width, _] = size.cells();
    let lead = (facts.len() as u16).saturating_sub(1) / 2;
    Plan {
        size,
        will: [body_x, body_y],
        facts: [body_x + width + GAP, body_y + size.eye_row().saturating_sub(lead)],
        card: [card_x, card_y, card_width, (body_rows + 2).min(rows.saturating_sub(card_y))],
        value_width: usize::from(value_width),
    }
}

/// Milliseconds from `elapsed` until Will's next frame.
pub(crate) const fn frame_wait(elapsed: u64) -> u64 {
    FRAME_MS - elapsed % FRAME_MS
}

/// The area the lockup and its composer share.
pub(crate) fn window_area(cols: u16, rows: u16, facts: &[(&str, String)]) -> [u16; 4] {
    plan(cols, rows, facts).card
}

/// The colour behind the lockup for `splash`.
pub(crate) fn ground(splash: Splash) -> Color {
    match splash {
        Splash::Sky => stage().plate,
        Splash::Atmosphere => stage().void,
        Splash::Off | Splash::Classic | Splash::Plain | Splash::Motes => palette().base,
    }
}

pub(crate) fn compose(cols: u16, rows: u16, app: &App, elapsed: u64, splash: Splash) -> Composed {
    let model = app.model.get();
    let facts = super::logo::facts(&model, app);
    let plan = plan(cols, rows, &facts);
    let mut plate = match splash {
        Splash::Sky => glass(plan.card),
        Splash::Atmosphere => {
            let mut blank = Cell::blank();
            blank.bg = ink(stage().void);
            Plate::new(plan.card, blank)
        }
        Splash::Off | Splash::Classic | Splash::Plain | Splash::Motes => {
            Plate::new(plan.card, Cell::blank())
        }
    };
    let tone = ground(splash);
    let colours = stage();
    let revealed =
        app.splash_revealed.peek() || !model.prefs.animations || !app.draft.peek().is_empty();
    let elapsed = if revealed { u64::MAX / 2 } else { elapsed };
    let fade = Easing::SineOut
        .apply(elapsed.saturating_sub(ENTRANCE_MS).min(FADE_MS) as f32 / FADE_MS as f32);
    let [x, y] = plan.facts;
    if fade > 0.0 {
        for (row, (label, value)) in facts.iter().enumerate() {
            let y = y + row as u16;
            if row == 0 {
                overlay(&mut plate, x, y, label, blend(tone, palette().text, fade));
                for col in 0..label.chars().count() as u16 {
                    if let Some(cell) = plate.cell([x + col, y]) {
                        cell.bold = true;
                    }
                }
                let offset = label.chars().count() as u16 + 1;
                overlay(&mut plate, x + offset, y, value, blend(tone, colours.label, fade));
                continue;
            }
            overlay(&mut plate, x, y, label, blend(tone, colours.label, fade));
            let value = crate::cells::ellipsize_start(value, plan.value_width);
            overlay(&mut plate, x + FACT_LABEL, y, &value, blend(tone, colours.value, fade));
        }
    }
    let figure = Figure {
        size: plan.size,
        at: plan.will,
        tint: model.prefs.will_tint,
        light: palette().light(),
        still: !model.prefs.animations,
        motes: splash == Splash::Motes,
        revealed,
    };
    drop(model);
    Composed {
        plate,
        shimmer: None,
        will: Some(figure),
    }
}

/// Will's place on the splash and how to draw each frame of his idle loop.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Figure {
    size: Size,
    at: [u16; 2],
    tint: Tint,
    light: bool,
    still: bool,
    motes: bool,
    revealed: bool,
}

impl Figure {
    /// Paints the frame for `elapsed` onto `plate`, restoring cells from `still` first, and records them in `lit`.
    pub(crate) fn apply(&self, plate: &mut Plate, still: &Plate, elapsed: u64, lit: &mut Vec<[u16; 2]>) {
        let pixels = super::sprite::frame(self.size, elapsed / FRAME_MS, self.still);
        let ink = crate::will::will_ink(self.tint, self.light);
        let paint = |pixel: Pixel| match pixel {
            Pixel::Empty => None,
            Pixel::Edge => Some(ink.edge),
            Pixel::Mid => Some(ink.mid),
            Pixel::Core => Some(ink.core),
            Pixel::Eye => Some(ink.eye),
        };
        let [width, rows] = self.size.cells();
        let shown = if self.revealed {
            0
        } else {
            let left = 1.0 - (elapsed.min(ENTRANCE_MS) as f32 / ENTRANCE_MS as f32);
            (f32::from(rows) * left).ceil() as u16
        };
        let mut cells: Vec<([u16; 2], Option<Color>, Option<Color>)> = Vec::new();
        for row in 0..rows {
            for col in 0..width {
                let pick = |half: usize| {
                    pixels
                        .get(usize::from(row) * 2 + half)
                        .and_then(|line| line.get(usize::from(col)))
                        .copied()
                        .and_then(paint)
                };
                let visible = row >= shown;
                let (top, bottom) = if visible { (pick(0), pick(1)) } else { (None, None) };
                cells.push(([self.at[0] + col, self.at[1] + row], top, bottom));
            }
        }
        if self.motes && !self.still {
            let frame = elapsed / FRAME_MS / MOTE_STEP;
            let width = i32::from(width);
            let spots = [[-3, 3], [-2, 10], [-3, 14], [1, -2], [width - 2, -1], [-1, 18]];
            for (n, [dx, dy]) in spots.into_iter().enumerate() {
                let x = i32::from(self.at[0]) + dx;
                let half = i32::from(self.at[1]) * 2 + dy;
                if (frame + n as u64 * MOTE_PHASE) % 8 >= 3 || x < 0 || half < 0 {
                    continue;
                }
                let at = [x as u16, (half / 2) as u16];
                let colour = Some(ink.mid);
                cells.push(if half % 2 == 0 { (at, colour, None) } else { (at, None, colour) });
            }
        }
        for (at, top, bottom) in cells {
            let (Some(cell), Some(base)) = (plate.cell(at), still.at(at)) else {
                continue;
            };
            cell.clone_from(base);
            lit.push(at);
            match (top, bottom) {
                (None, None) => {}
                (Some(top), None) => {
                    cell.set_text("▀");
                    cell.fg = super::ink(top);
                }
                (None, Some(bottom)) => {
                    cell.set_text("▄");
                    cell.fg = super::ink(bottom);
                }
                (Some(top), Some(bottom)) => {
                    cell.set_text("▀");
                    cell.fg = super::ink(top);
                    cell.bg = super::ink(bottom);
                }
            }
        }
    }
}

/// Paints the clear field around the lockup on the atmosphere splash: full void over `area`, fading out over the sky.
pub(crate) fn veil(scene: &mut SceneBuilder, frame: Rect, (cols, rows): (u16, u16), area: [u16; 4]) {
    let cell = metrics();
    let void = stage().void;
    let [left, top, width, height] = area.map(f32::from);
    let height = height + f32::from(COMPOSER_ROWS);
    for y in 0..rows {
        for x in 0..cols {
            let (cx, cy) = (f32::from(x) + 0.5, f32::from(y) + 0.5);
            let dx = (left - cx).max(cx - (left + width)).max(0.0) / VEIL[0];
            let dy = (top - cy).max(cy - (top + height)).max(0.0) / VEIL[1];
            let reach = dx.hypot(dy).min(1.0);
            let alpha = 1.0 - reach * reach * (3.0 - 2.0 * reach);
            if alpha <= 0.01 {
                continue;
            }
            let rect = Rect::new(
                Point::new(
                    frame.origin.x + f32::from(x) * cell.w,
                    frame.origin.y + f32::from(y) * cell.h,
                ),
                cell.w,
                cell.h,
            );
            scene.draw_rect(DrawRect::new(
                rect,
                Color {
                    a: (alpha * 255.0) as u8,
                    ..void
                },
            ));
        }
    }
}

const VEIL: [f32; 2] = [12.0, 5.0];
