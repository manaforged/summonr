use super::input::{glass, overlay};
use super::logo::{
    COMPOSER_ROWS, Composed, FACT_LABEL, FADE_MS, WORD_GAP, WORD_WIPE_MS, layout, wordmark,
};
use super::space::Plate;
use super::sprite::{FRAME_MS, Pixel, Size};
use super::*;
use sigil::animation::Easing;
use summonr_agent::prefs::{Splash, Tint};

const GAP: u16 = 4;
const ENTRANCE_MS: u64 = 600;
const WORD_START_MS: u64 = ENTRANCE_MS / 2;
const MOTE_STEP: u64 = 2;
const MOTE_PHASE: u64 = 3;
const SCALES: [u16; 3] = [3, 2, 1];

/// Whether `splash` draws the pixel Will lockup rather than the classic mark.
pub(crate) const fn drawn(splash: Splash) -> bool {
    !matches!(splash, Splash::Off | Splash::Classic)
}

/// Milliseconds until the lockup has finished its entrance.
pub(crate) const fn reveal_ms() -> u64 {
    WORD_START_MS + WORD_WIPE_MS + 2 * FADE_MS
}

struct Plan {
    size: Size,
    scale: u16,
    will: [u16; 2],
    word: Option<[u16; 2]>,
    facts: [u16; 2],
    card: [u16; 4],
    value_width: usize,
}

struct Fit {
    fits: bool,
    body: [u16; 2],
    will_rows: u16,
    text_rows: u16,
    value_width: u16,
}

fn fit(cols: u16, rows: u16, facts: &[(&str, String)], size: Size, scale: u16, word: bool) -> Fit {
    let longest = facts
        .iter()
        .map(|(_, value)| value.chars().count() as u16)
        .max()
        .unwrap_or(20);
    let [width, height] = size.cells().map(|cells| cells * scale);
    let gap = GAP + scale;
    let room = cols.saturating_sub(width + gap + FACT_LABEL + 6);
    let value_width = longest.min(room);
    let logo = summonr_agent::splash::logo().len() as u16;
    let word_width = if word {
        summonr_agent::splash::logo_width() as u16
    } else {
        0
    };
    let text_width = word_width.max(FACT_LABEL + value_width);
    let text_rows = if word { logo + WORD_GAP } else { 0 } + facts.len() as u16;
    let body = [width + gap + text_width, height.max(text_rows)];
    let fits = body[1] + 2 + COMPOSER_ROWS <= rows
        && body[0] + 6 <= cols
        && (scale == 1 || height * 2 <= rows);
    Fit {
        fits,
        body,
        will_rows: height,
        text_rows,
        value_width,
    }
}

fn plan(cols: u16, rows: u16, facts: &[(&str, String)]) -> Plan {
    let choices = SCALES
        .into_iter()
        .flat_map(|scale| [(Size::Banner, scale, true), (Size::Banner, scale, false)])
        .chain([(Size::Compact, 1, false)]);
    let mut picked = (Size::Compact, 1, false);
    for (size, scale, word) in choices {
        if fit(cols, rows, facts, size, scale, word).fits {
            picked = (size, scale, word);
            break;
        }
    }
    let (size, scale, word) = picked;
    let fit = fit(cols, rows, facts, size, scale, word);
    let [body_width, body_rows] = fit.body;
    let [body_x, body_y, card_x, card_y, card_width] = layout(cols, rows, body_width, body_rows);
    let [width, _] = size.cells().map(|cells| cells * scale);
    let text_x = body_x + width + GAP + scale;
    let text_y = body_y + body_rows.saturating_sub(fit.text_rows) / 2;
    let logo = summonr_agent::splash::logo().len() as u16;
    Plan {
        size,
        scale,
        will: [body_x, body_y + body_rows.saturating_sub(fit.will_rows)],
        word: word.then_some([text_x, text_y]),
        facts: [
            text_x,
            if word {
                text_y + logo + WORD_GAP
            } else {
                text_y
            },
        ],
        card: [
            card_x,
            card_y,
            card_width,
            (body_rows + 2).min(rows.saturating_sub(card_y)),
        ],
        value_width: usize::from(fit.value_width),
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

/// The one solid colour the lockup card and composer share, when `splash` has no glass.
pub(crate) fn panel(splash: Splash) -> Option<Color> {
    match splash {
        Splash::Plain | Splash::Motes | Splash::Atmosphere => {
            Some(crate::theme::backdrop(palette()))
        }
        Splash::Off | Splash::Classic | Splash::Sky => None,
    }
}

/// The colour behind the lockup for `splash`.
pub(crate) fn ground(splash: Splash) -> Color {
    match splash {
        Splash::Sky => stage().plate,
        Splash::Atmosphere => crate::theme::backdrop(palette()),
        Splash::Off | Splash::Classic | Splash::Plain | Splash::Motes => palette().base,
    }
}

pub(crate) fn compose(cols: u16, rows: u16, app: &App, elapsed: u64, splash: Splash) -> Composed {
    let model = app.model.get();
    let facts = super::logo::facts(&model, app);
    let plan = plan(cols, rows, &facts);
    let tone = ground(splash);
    let mut plate = match splash {
        Splash::Sky => glass(plan.card),
        Splash::Atmosphere => {
            let mut blank = Cell::blank();
            blank.bg = ink(tone);
            Plate::new(plan.card, blank)
        }
        Splash::Off | Splash::Classic | Splash::Plain | Splash::Motes => {
            Plate::new(plan.card, Cell::blank())
        }
    };
    let colours = stage();
    let revealed =
        app.splash_revealed.peek() || !model.prefs.animations || !app.draft.peek().is_empty();
    let elapsed = if revealed { u64::MAX / 2 } else { elapsed };
    if let Some(at) = plan.word {
        wordmark(&mut plate, at, elapsed.saturating_sub(WORD_START_MS), tone);
    }
    let facts_start = if plan.word.is_some() {
        WORD_START_MS + WORD_WIPE_MS
    } else {
        ENTRANCE_MS
    };
    let fade = Easing::SineOut
        .apply(elapsed.saturating_sub(facts_start).min(FADE_MS) as f32 / FADE_MS as f32);
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
                overlay(
                    &mut plate,
                    x + offset,
                    y,
                    value,
                    blend(tone, colours.label, fade),
                );
                continue;
            }
            overlay(&mut plate, x, y, label, blend(tone, colours.label, fade));
            let value = crate::cells::ellipsize_start(value, plan.value_width);
            overlay(
                &mut plate,
                x + FACT_LABEL,
                y,
                &value,
                blend(tone, colours.value, fade),
            );
        }
    }
    let figure = Figure {
        size: plan.size,
        scale: plan.scale,
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
    scale: u16,
    at: [u16; 2],
    tint: Tint,
    light: bool,
    still: bool,
    motes: bool,
    revealed: bool,
}

type Painted = ([u16; 2], Option<Color>, Option<Color>);

impl Figure {
    /// Paints the frame for `elapsed` onto `plate`, restoring cells from `still` first, and records them in `lit`.
    pub(crate) fn apply(
        &self,
        plate: &mut Plate,
        still: &Plate,
        elapsed: u64,
        lit: &mut Vec<[u16; 2]>,
    ) {
        let mut cells = self.sprite(elapsed);
        if self.motes && !self.still {
            cells.extend(self.motes(elapsed));
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

    fn colour(&self, pixel: Pixel) -> Option<Color> {
        let ink = crate::will::will_ink(self.tint, self.light);
        match pixel {
            Pixel::Empty => None,
            Pixel::Edge => Some(ink.edge),
            Pixel::Mid => Some(ink.mid),
            Pixel::Core => Some(ink.core),
            Pixel::Eye => Some(ink.eye),
        }
    }

    fn sprite(&self, elapsed: u64) -> Vec<Painted> {
        let pixels = super::sprite::frame(self.size, elapsed / FRAME_MS, self.still);
        let scale = usize::from(self.scale);
        let [width, rows] = self.size.cells().map(|cells| cells * self.scale);
        let hidden = if self.revealed {
            0
        } else {
            let left = 1.0 - (elapsed.min(ENTRANCE_MS) as f32 / ENTRANCE_MS as f32);
            (f32::from(rows) * left).ceil() as u16
        };
        let pick = |half: usize, col: usize| {
            pixels
                .get(half / scale)
                .and_then(|line| line.get(col / scale))
                .copied()
                .and_then(|pixel| self.colour(pixel))
        };
        let mut cells = Vec::with_capacity(usize::from(width) * usize::from(rows));
        for row in 0..rows {
            for col in 0..width {
                let at = [self.at[0] + col, self.at[1] + row];
                if row < hidden {
                    cells.push((at, None, None));
                    continue;
                }
                let half = usize::from(row) * 2;
                let col = usize::from(col);
                cells.push((at, pick(half, col), pick(half + 1, col)));
            }
        }
        cells
    }

    fn motes(&self, elapsed: u64) -> Vec<Painted> {
        let frame = elapsed / FRAME_MS / MOTE_STEP;
        let scale = i32::from(self.scale);
        let [width, _] = self.size.cells();
        let width = i32::from(width);
        let colour = self.colour(Pixel::Mid);
        let spots = [
            [-3, 3],
            [-2, 10],
            [-3, 14],
            [1, -2],
            [width - 2, -1],
            [-1, 18],
        ];
        let mut cells = Vec::new();
        for (n, [dx, dy]) in spots.into_iter().enumerate() {
            let x = i32::from(self.at[0]) + dx * scale;
            let half = i32::from(self.at[1]) * 2 + dy * scale;
            if (frame + n as u64 * MOTE_PHASE) % 8 >= 3 || x < 0 || half < 0 {
                continue;
            }
            for step in 0..scale {
                let half = half + step;
                for x in x..x + scale {
                    let at = [x as u16, (half / 2) as u16];
                    cells.push(if half % 2 == 0 {
                        (at, colour, None)
                    } else {
                        (at, None, colour)
                    });
                }
            }
        }
        cells
    }
}
