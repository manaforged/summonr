use super::input::{FACTS_MS, ROW_MS, ROW_WIPE_MS, clear, dim_rect, overlay, prompt, write};
use super::space::STAR_BRIGHT;
use super::*;
use sigil::prelude::Paint;

pub(in crate::tui::sigil_view) fn compose(
    cells: &mut [Cell],
    cols: u16,
    rows: u16,
    app: &App,
    ticks: u64,
) {
    let width = usize::from(cols);
    if width == 0 || rows == 0 {
        return;
    }
    let model = app.model.get();
    let facts = facts(&model, app);
    let logo = crate::splash::logo();
    let [body_width, body_rows] = cover(logo, &facts);
    let [body_x, body_y, input_x, input_y, input_width] = layout(cols, rows, body_width, body_rows);
    let scrim_x = body_x.saturating_sub(2);
    let scrim_y = body_y.saturating_sub(1);
    dim_rect(
        cells,
        cols,
        [
            scrim_x,
            scrim_y,
            (body_width + 4).min(cols.saturating_sub(scrim_x)),
            (body_rows + 2).min(rows.saturating_sub(scrim_y)),
        ],
        true,
    );
    let accent = palette().accent;
    let revealed =
        app.splash_revealed.peek() || !model.prefs.animations || !app.draft.peek().is_empty();
    let elapsed = if revealed {
        u64::MAX / 2
    } else {
        u64::try_from(app.splash_since.get().elapsed().as_millis()).unwrap_or(u64::MAX / 2)
    };
    let facts_visible = elapsed >= FACTS_MS;
    let logo_width = crate::splash::logo_width() as u16;
    let logo_x = body_x + body_width.saturating_sub(logo_width) / 2;
    let facts_x = body_x;
    for (row, text) in logo.iter().enumerate() {
        let width = text.chars().count();
        let shown = reveal_chars(row, width, elapsed);
        for (col, glyph) in text.chars().take(shown).enumerate() {
            if glyph != ' ' {
                overlay(
                    cells,
                    cols,
                    logo_x + col as u16,
                    body_y + row as u16,
                    &glyph.to_string(),
                    bloom(row, col, width, elapsed, accent),
                );
            }
        }
    }
    let facts_y = body_y + logo.len() as u16 + 1;
    for (row, (label, value)) in facts.iter().enumerate().filter(|_| facts_visible) {
        write(
            cells,
            cols,
            facts_x,
            facts_y + row as u16,
            &format!("{label:<8} "),
            palette().dim,
        );
        write(
            cells,
            cols,
            facts_x + 9,
            facts_y + row as u16,
            value,
            palette().gray,
        );
    }
    clear(
        cells,
        cols,
        input_x,
        input_y.saturating_sub(1),
        input_width,
        4,
    );
    prompt(
        cells,
        cols,
        [input_x, input_y, input_width],
        &model,
        app,
        facts_visible,
        ticks,
    );
    drop(model);
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
        ("setup", "/login · /model · /help".to_string()),
    ];
    if model.snapshot.plan {
        facts.push(("mode", "plan · read-only".to_string()));
    }
    if model.snapshot.work {
        facts.push(("mode", "work · todos and verify".to_string()));
    }
    if model.snapshot.strict {
        facts.push(("policy", "strict".to_string()));
    }
    facts
}

pub(in crate::tui::sigil_view) fn cover(logo: &[&str; 6], facts: &[(&str, String)]) -> [u16; 2] {
    let logo_width = crate::splash::logo_width() as u16;
    let facts_width = facts
        .iter()
        .map(|(label, value)| format!("{label:<8} {value}").chars().count() as u16)
        .max()
        .unwrap_or(20);
    [
        logo_width.max(facts_width),
        logo.len() as u16 + facts.len() as u16 + 2,
    ]
}

pub(in crate::tui::sigil_view) fn layout(
    cols: u16,
    rows: u16,
    body_width: u16,
    body_rows: u16,
) -> [u16; 5] {
    let gutter = if cols > 40 {
        16
    } else if cols > 28 {
        8
    } else {
        2
    };
    let input_width = cols.saturating_sub(gutter).min(72);
    let total = body_rows + 5;
    let body_x = cols.saturating_sub(body_width) / 2;
    let input_x = cols.saturating_sub(input_width) / 2;
    let (body_y, input_y) = if rows >= total + 2 {
        let top = rows.saturating_sub(total) / 2;
        (top, top + body_rows + 2)
    } else {
        (rows.saturating_sub(body_rows) / 2, rows.saturating_sub(3))
    };
    [body_x, body_y, input_x, input_y, input_width]
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
    accent: Rgba8,
) -> Rgba8 {
    let appeared = row as u64 * ROW_MS + col as u64 * ROW_WIPE_MS / width.max(1) as u64;
    let age = elapsed.saturating_sub(appeared);
    if age >= 450 {
        accent
    } else if age >= 150 {
        rgb(120, 148, 200)
    } else {
        STAR_BRIGHT
    }
}

pub(in crate::tui::sigil_view) fn scene_row(cells: &[Cell]) -> NodeBuilder {
    let mut row = container().layout(
        LayoutStyle::flex_row()
            .height(metrics().h)
            .fill_width()
            .gap(0.0),
    );
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
        let text: String = cells[start..end]
            .iter()
            .map(|cell| cell.text.as_str())
            .collect();
        let surface = shown(cell.bg, palette().base);
        let ink = shown(cell.fg, palette().text);
        let weight = if cell.bold { 700 } else { 400 };
        row = row.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .width((end - start) as f32 * metrics().w)
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
                ),
        );
        start = end;
    }
    row
}
