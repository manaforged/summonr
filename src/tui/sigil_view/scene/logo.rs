use super::input::{FACTS_MS, ROW_MS, ROW_WIPE_MS, glass, overlay};
use super::space::{PLATE, STAR_BRIGHT};
use super::*;
use sigil::prelude::Paint;

const COMPOSER_ROWS: u16 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Card {
    Lockup,
    Mark,
    Banner,
}

impl Card {
    const fn mark(self) -> super::Mark {
        match self {
            Self::Lockup | Self::Mark => super::Mark::Lines,
            Self::Banner => super::Mark::Banner,
        }
    }
}

const LOCKUP_GAP: u16 = 5;
const WORD_GAP: u16 = 2;
const LOCKUP_MARGIN: u16 = 10;
const FACT_LABEL: u16 = 9;

struct Placed {
    mark: [u16; 2],
    word: Option<[u16; 2]>,
    facts: [u16; 2],
}

fn place(card: Card, [body_x, body_y]: [u16; 2], [body_width, body_rows]: [u16; 2]) -> Placed {
    let [mark_width, mark_rows] = super::mark::size(card.mark());
    if card == Card::Lockup {
        let word_rows = crate::splash::logo().len() as u16;
        let column = word_rows + WORD_GAP;
        let top = body_y + body_rows.saturating_sub(column + 4) / 2;
        let right = body_x + mark_width + LOCKUP_GAP;
        return Placed {
            mark: [body_x, body_y + body_rows.saturating_sub(mark_rows) / 2],
            word: Some([right, top]),
            facts: [right, top + column],
        };
    }
    Placed {
        mark: [body_x + body_width.saturating_sub(mark_width) / 2, body_y],
        word: (card == Card::Banner)
            .then_some([body_x + body_width.saturating_sub(mark_width) / 2, body_y]),
        facts: [body_x, body_y + mark_rows + 1],
    }
}

pub(in crate::tui::sigil_view) fn compose(
    cells: &mut [Cell],
    cols: u16,
    rows: u16,
    app: &App,
) -> [u16; 4] {
    let width = usize::from(cols);
    if width == 0 || rows == 0 {
        return [0; 4];
    }
    let model = app.model.get();
    let facts = facts(&model, app);
    let card_kind = fitting(cols, rows, &facts);
    let [body_width, body_rows] = cover(card_kind, &facts);
    let [body_x, body_y, card_x, card_y, card_width] = layout(cols, rows, body_width, body_rows);
    let card = [
        card_x + 1,
        card_y + 1,
        card_width.saturating_sub(2),
        (body_rows + 1).min(rows.saturating_sub(card_y + 1)),
    ];
    glass(cells, cols, card);
    let revealed =
        app.splash_revealed.peek() || !model.prefs.animations || !app.draft.peek().is_empty();
    let elapsed = if revealed {
        u64::MAX / 2
    } else {
        u64::try_from(app.splash_since.get().elapsed().as_millis()).unwrap_or(u64::MAX / 2)
    };
    let placed = place(card_kind, [body_x, body_y], [body_width, body_rows]);
    let value_width = usize::from(match card_kind {
        Card::Lockup => (crate::splash::logo_width() as u16).saturating_sub(FACT_LABEL),
        Card::Mark | Card::Banner => body_width.saturating_sub(FACT_LABEL),
    });
    for (row, (label, value)) in facts.iter().enumerate().filter(|_| elapsed >= FACTS_MS) {
        let [x, y] = placed.facts;
        let y = y + row as u16;
        overlay(cells, cols, x, y, label, palette().dim);
        let value = crate::text::ellipsize_start(value, value_width);
        overlay(cells, cols, x + FACT_LABEL, y, &value, palette().gray);
    }
    if let Some(at) = placed.word {
        wordmark(cells, cols, at, elapsed);
    }
    if card_kind.mark() == super::Mark::Lines {
        let clock = model.prefs.animations.then(|| {
            u64::try_from(app.splash_since.get().elapsed().as_millis()).unwrap_or(u64::MAX / 2)
        });
        super::mark::paint(cells, cols, placed.mark, super::Mark::Lines, elapsed, clock);
    }
    drop(model);
    card
}

fn wordmark(cells: &mut [Cell], cols: u16, [x, y]: [u16; 2], elapsed: u64) {
    let face = palette().text;
    for (row, text) in crate::splash::logo().iter().enumerate() {
        let width = text.chars().count();
        let shown = reveal_chars(row, width, elapsed);
        for (col, glyph) in text.chars().take(shown).enumerate() {
            let at = [x + col as u16, y + row as u16];
            let ink = bloom(row, col, width, elapsed, face);
            if glyph == SOLID {
                solid(cells, cols, at, ink);
            } else if glyph != ' ' {
                overlay(
                    cells,
                    cols,
                    at[0],
                    at[1],
                    &glyph.to_string(),
                    blend(ink, PLATE, SHADE),
                );
            }
        }
    }
}

const SOLID: char = '█';
const SHADE: f32 = 0.45;

fn solid(cells: &mut [Cell], cols: u16, [x, y]: [u16; 2], ink: Rgba8) {
    let index = usize::from(y) * usize::from(cols) + usize::from(x);
    if let Some(cell) = cells.get_mut(index) {
        cell.text = " ".into();
        cell.bg = super::ink(ink);
    }
}

pub(in crate::tui::sigil_view) fn facts(model: &Model, app: &App) -> Vec<(&'static str, String)> {
    let shown_model = model.snapshot.relay.as_deref().map_or_else(
        || model.snapshot.model.clone(),
        |relay| format!("{} · {relay}", model.snapshot.model),
    );
    let mut facts = vec![
        ("summonr", app.info.version.clone()),
        ("model", shown_model),
        ("session", crate::session::short(&model.session).to_string()),
        ("workdir", app.info.workspace.clone()),
    ];
    if app.unseated() {
        facts.push(("setup", "/login · /model · /help".to_string()));
    }
    if model.snapshot.work {
        facts.push(("mode", "work · steps and verify".to_string()));
    }
    if model.snapshot.strict {
        facts.push(("policy", "strict".to_string()));
    }
    facts
}

pub(in crate::tui::sigil_view) fn fitting(cols: u16, rows: u16, facts: &[(&str, String)]) -> Card {
    let fits = |card: Card| cover(card, facts)[1] + 2 + COMPOSER_ROWS <= rows;
    if fits(Card::Lockup) && cover(Card::Lockup, facts)[0] + LOCKUP_MARGIN <= cols {
        Card::Lockup
    } else if fits(Card::Mark) {
        Card::Mark
    } else {
        Card::Banner
    }
}

pub(in crate::tui::sigil_view) fn cover(card: Card, facts: &[(&str, String)]) -> [u16; 2] {
    let [logo_width, logo_rows] = super::mark::size(card.mark());
    let facts_width = facts
        .iter()
        .map(|(label, value)| format!("{label:<8} {value}").chars().count() as u16)
        .max()
        .unwrap_or(20);
    if card == Card::Lockup {
        let word_width = crate::splash::logo_width() as u16;
        let word_rows = crate::splash::logo().len() as u16;
        return [
            logo_width + LOCKUP_GAP + word_width,
            logo_rows.max(word_rows + WORD_GAP + facts.len() as u16),
        ];
    }
    [
        logo_width.max(facts_width),
        logo_rows + facts.len() as u16 + 2,
    ]
}

pub(in crate::tui::sigil_view) fn layout(
    cols: u16,
    rows: u16,
    body_width: u16,
    body_rows: u16,
) -> [u16; 5] {
    let top = rows.saturating_sub(body_rows + 2 + COMPOSER_ROWS) / 2;
    let body_x = cols.saturating_sub(body_width) / 2;
    let card_x = body_x.saturating_sub(3);
    [
        body_x,
        top + 2,
        card_x,
        top,
        (body_width + 6).min(cols.saturating_sub(card_x)),
    ]
}

pub(in crate::tui::sigil_view) fn reveal_chars(row: usize, len: usize, elapsed: u64) -> usize {
    let start = row as u64 * ROW_MS;
    if elapsed <= start {
        0
    } else if elapsed >= start + ROW_WIPE_MS {
        len
    } else {
        len * (elapsed - start) as usize / ROW_WIPE_MS as usize
    }
}

pub(in crate::tui::sigil_view) fn bloom(
    row: usize,
    col: usize,
    width: usize,
    elapsed: u64,
    settled: Rgba8,
) -> Rgba8 {
    let appeared = row as u64 * ROW_MS + col as u64 * ROW_WIPE_MS / width.max(1) as u64;
    let age = elapsed.saturating_sub(appeared);
    if age >= 450 {
        settled
    } else if age >= 150 {
        blend(STAR_BRIGHT, settled, 0.5)
    } else {
        STAR_BRIGHT
    }
}

pub(in crate::tui::sigil_view) fn placed(x: u16, y: u16, cells: &[Cell]) -> NodeBuilder {
    runs(
        container().layout(
            LayoutStyle::flex_row()
                .position(Position::Absolute)
                .inset_left(f32::from(x) * metrics().w)
                .inset_top(f32::from(y) * metrics().h)
                .width(cells.len() as f32 * metrics().w)
                .height(metrics().h)
                .gap(0.0),
        ),
        cells,
    )
}

fn runs(mut parent: NodeBuilder, cells: &[Cell]) -> NodeBuilder {
    let mut start = 0;
    while start < cells.len() {
        let cell = &cells[start];
        let mut end = start + 1;
        while end < cells.len()
            && cells[end].fg == cell.fg
            && cells[end].bg == cell.bg
            && cells[end].bold == cell.bold
            && cells[end].italic == cell.italic
        {
            end += 1;
        }
        parent = parent.child(run(&cells[start..end]));
        start = end;
    }
    parent
}

fn run(cells: &[Cell]) -> NodeBuilder {
    let cell = &cells[0];
    let text: String = cells.iter().map(|cell| cell.text.as_str()).collect();
    let surface = shown(cell.bg, palette().base);
    let ink = shown(cell.fg, palette().text);
    let weight = if cell.bold { 700 } else { 400 };
    container()
        .layout(
            LayoutStyle::flex_row()
                .width(cells.len() as f32 * metrics().w)
                .height(metrics().h),
        )
        .bg(surface)
        .child(
            sigil::kernel::text(text)
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_family(sigil::prelude::FontFamily::Mono)
                .font_weight(weight)
                .text_color(ink),
        )
}
