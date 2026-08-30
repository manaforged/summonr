use sigil::prelude::*;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::App;
use super::canvas::{BASE, CYAN, Cell, DIM, GRAY, GREEN, TEXT, cell as metrics, rgb};
use super::model::Model;
use crate::tui::short_session;

const SPACE: Rgba8 = rgb(8, 12, 28);
const SCRIM: Rgba8 = rgb(10, 8, 20);
const STAR_DIM: Rgba8 = rgb(58, 74, 110);
const STAR_MID: Rgba8 = rgb(120, 148, 200);
const STAR_BRIGHT: Rgba8 = rgb(176, 200, 240);

const SKY: [Rgba8; 5] = [
    rgb(24, 15, 46),
    rgb(56, 32, 84),
    rgb(98, 48, 102),
    rgb(158, 74, 98),
    rgb(222, 110, 74),
];
const FANTASY_STAR_DIM: Rgba8 = rgb(108, 100, 142);
const STAR_LIT: Rgba8 = rgb(240, 230, 200);
const CLOUD: Rgba8 = rgb(64, 34, 80);
const CLOUD_LIT: Rgba8 = rgb(214, 120, 100);
const SUN: Rgba8 = rgb(255, 214, 110);
const SUN_MID: Rgba8 = rgb(246, 156, 84);
const SUN_EDGE: Rgba8 = rgb(214, 104, 74);
const FAR: Rgba8 = rgb(126, 70, 104);
const FAR_RIM: Rgba8 = rgb(240, 160, 122);
const NEAR: Rgba8 = rgb(34, 20, 52);
const CASTLE: Rgba8 = rgb(22, 13, 36);
const WINDOW: Rgba8 = rgb(255, 224, 130);
const PENNANT: Rgba8 = rgb(255, 106, 94);
const GROUND: Rgba8 = rgb(26, 52, 38);
const GROUND_DARK: Rgba8 = rgb(18, 38, 27);
const GRASS: Rgba8 = rgb(44, 84, 56);
const FLOWER: Rgba8 = rgb(226, 178, 106);
const FLY: Rgba8 = rgb(255, 232, 154);
const ROW_MS: u64 = 70;
const ROW_WIPE_MS: u64 = 130;
const FACTS_MS: u64 = 560;

const FAR_RIDGE: &[(f32, f32)] = &[
    (0.00, 0.58),
    (0.10, 0.64),
    (0.24, 0.70),
    (0.42, 0.74),
    (0.60, 0.68),
    (0.78, 0.60),
    (1.00, 0.56),
];

const NEAR_RIDGE: &[(f32, f32)] = &[
    (0.00, 0.72),
    (0.16, 0.78),
    (0.40, 0.82),
    (0.62, 0.78),
    (0.80, 0.70),
    (0.90, 0.66),
    (1.00, 0.70),
];

pub(super) fn layer(app: App) -> NodeBuilder {
    let (cols, rows) = app.viewport.get();
    let name = app.theme_name.get();
    let ticks = if app.model.borrow().prefs.animations {
        app.ticks.get()
    } else {
        0
    };
    let mut cells = if name == "fantasy" {
        fantasy(cols, rows, ticks)
    } else {
        space(cols, rows, ticks)
    };
    compose(&mut cells, cols, rows, &app, ticks);
    let mut layer = container().layout(
        LayoutStyle::flex_column()
            .position(Position::Absolute)
            .inset(0.0)
            .fill_width()
            .fill_height()
            .gap(0.0),
    );
    let width = usize::from(cols);
    for line in cells.chunks(width.max(1)) {
        layer = layer.child(scene_row(line));
    }
    layer
}

fn compose(cells: &mut [Cell], cols: u16, rows: u16, app: &App, ticks: u64) {
    let width = usize::from(cols);
    if width == 0 || rows == 0 {
        return;
    }
    let model = app.model.borrow();
    let name = app.theme_name.peek();
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
        name != "fantasy",
    );
    let accent = if name == "fantasy" {
        rgb(255, 208, 96)
    } else {
        rgb(105, 140, 210)
    };
    let revealed =
        app.splash_revealed.peek() || !model.prefs.animations || !app.draft.peek().is_empty();
    let elapsed = if revealed {
        u64::MAX / 2
    } else {
        ticks.saturating_mul(40)
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
                    bloom(&name, row, col, width, elapsed, accent),
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
            DIM,
        );
        write(cells, cols, facts_x + 9, facts_y + row as u16, value, GRAY);
    }
    clear(cells, cols, input_x, input_y, input_width, 3);
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
    if let Some(menu) = super::suggest::menu(app, cols, rows) {
        let left = menu.left as u16;
        let top = menu.top as u16;
        let width = menu
            .lines
            .first()
            .map(|line| UnicodeWidthStr::width(line.as_str()) as u16)
            .unwrap_or(0);
        clear(cells, cols, left, top, width, menu.lines.len() as u16);
        for (offset, line) in menu.lines.iter().enumerate() {
            write(
                cells,
                cols,
                left,
                top.saturating_add(offset as u16),
                line,
                TEXT,
            );
        }
    }
}

pub(super) fn prompt_area(app: &App, cols: u16, rows: u16) -> [u16; 3] {
    let model = app.model.borrow();
    let facts = facts(&model, app);
    let [body_width, body_rows] = cover(crate::splash::logo(), &facts);
    let [_, _, input_x, input_y, input_width] = layout(cols, rows, body_width, body_rows);
    [input_x, input_y, input_width]
}

fn facts(model: &Model, app: &App) -> Vec<(&'static str, String)> {
    let shown_model = model.relay.as_deref().map_or_else(
        || model.model.clone(),
        |relay| format!("{} · {relay}", model.model),
    );
    let mut facts = vec![
        ("summoner", app.info.version.clone()),
        ("model", shown_model),
        ("session", short_session(&app.info.session)),
        ("workdir", app.info.workspace.clone()),
    ];
    if model.snapshot.plan {
        facts.push(("mode", "plan · read-only".to_string()));
    }
    if model.snapshot.strict {
        facts.push(("policy", "strict".to_string()));
    }
    facts
}

fn cover(logo: &[&str; 6], facts: &[(&str, String)]) -> [u16; 2] {
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

fn layout(cols: u16, rows: u16, body_width: u16, body_rows: u16) -> [u16; 5] {
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

fn reveal_chars(row: usize, len: usize, elapsed: u64) -> usize {
    let start = row as u64 * ROW_MS;
    if elapsed <= start {
        0
    } else if elapsed >= start + ROW_WIPE_MS {
        len
    } else {
        len * (elapsed - start) as usize / ROW_WIPE_MS as usize
    }
}

fn bloom(theme: &str, row: usize, col: usize, width: usize, elapsed: u64, accent: Rgba8) -> Rgba8 {
    let appeared = row as u64 * ROW_MS + col as u64 * ROW_WIPE_MS / width.max(1) as u64;
    let age = elapsed.saturating_sub(appeared);
    if age >= 450 {
        accent
    } else if age >= 150 {
        if theme == "fantasy" {
            rgb(255, 168, 72)
        } else {
            rgb(120, 148, 200)
        }
    } else if theme == "fantasy" {
        rgb(255, 240, 180)
    } else {
        STAR_BRIGHT
    }
}

fn dim_rect(cells: &mut [Cell], cols: u16, area: [u16; 4], reset_background: bool) {
    let [x, y, width, height] = area;
    let rows = u16::try_from(cells.len() / usize::from(cols.max(1))).unwrap_or(u16::MAX);
    for row in y..y.saturating_add(height).min(rows) {
        for col in x..x.saturating_add(width).min(cols) {
            let index = usize::from(row) * usize::from(cols) + usize::from(col);
            if let Some(cell) = cells.get_mut(index) {
                let reset = rgb(0, 0, 0);
                let fg = if reset_background && cell.ch == ' ' {
                    reset
                } else {
                    cell.fg
                };
                let bg = if reset_background { reset } else { cell.bg };
                cell.fg = blend(fg, SCRIM, 0.55);
                cell.bg = blend(bg, SCRIM, 0.55);
            }
        }
    }
}

fn overlay(cells: &mut [Cell], cols: u16, x: u16, y: u16, text: &str, color: Rgba8) {
    let mut col = x;
    for ch in text.chars() {
        if ch != ' ' {
            let bg = cells
                .get(usize::from(y) * usize::from(cols) + usize::from(col))
                .map_or(BASE, |cell| cell.bg);
            set(
                cells,
                cols,
                col,
                y,
                Cell {
                    ch,
                    fg: color,
                    bg,
                    bold: false,
                    italic: false,
                },
            );
        }
        col = col.saturating_add(UnicodeWidthChar::width(ch).unwrap_or(0) as u16);
    }
}

fn clear(cells: &mut [Cell], cols: u16, x: u16, y: u16, width: u16, height: u16) {
    for row in y..y.saturating_add(height) {
        for col in x..x.saturating_add(width) {
            set(cells, cols, col, row, Cell::blank());
        }
    }
}

fn prompt(
    cells: &mut [Cell],
    cols: u16,
    area: [u16; 3],
    model: &Model,
    app: &App,
    visible: bool,
    ticks: u64,
) {
    let [x, y, width] = area;
    let mode = if model.snapshot.plan { "plan" } else { "auto" };
    let glyph = if model.snapshot.plan { '◇' } else { '●' };
    let mode_color = if model.snapshot.plan { CYAN } else { GREEN };
    let busy = model.busy_since.is_some();
    let accent = if model.prefs.theme == "fantasy" {
        rgb(255, 208, 96)
    } else {
        rgb(105, 140, 210)
    };
    let border = if busy { accent } else { GRAY };
    let right_width = usize::from(width).saturating_sub(6 + mode.len());
    let right = super::chat::ellipsize(&model.model, right_width);
    let fill =
        usize::from(width).saturating_sub(6 + mode.len() + UnicodeWidthStr::width(right.as_str()));
    write(
        cells,
        cols,
        x,
        y,
        &format!("╭ {glyph} {mode} {}", "─".repeat(fill)),
        border,
    );
    write(
        cells,
        cols,
        x + 1,
        y,
        &format!(" {glyph} {mode} "),
        mode_color,
    );
    write(
        cells,
        cols,
        x + width.saturating_sub(UnicodeWidthStr::width(right.as_str()) as u16 + 2),
        y,
        &right,
        DIM,
    );
    write(cells, cols, x + width.saturating_sub(1), y, "╮", border);
    write(cells, cols, x, y + 1, "│", border);
    if visible && app.view_edit.peek() {
        let path = app
            .model
            .borrow()
            .view
            .as_ref()
            .map(|view| view.path.clone())
            .unwrap_or_default();
        write(
            cells,
            cols,
            x + 3,
            y + 1,
            &format!("editing {path} · ctrl+s save · esc leave"),
            DIM,
        );
    } else if visible {
        write(cells, cols, x + 3, y + 1, "❯ ", mode_color);
        let caret = if (ticks / 6).is_multiple_of(2) {
            '▌'
        } else {
            ' '
        };
        let input = super::chat::draft_view(
            &app.draft.peek(),
            app.caret.peek(),
            usize::from(width.saturating_sub(8)),
            caret,
        );
        write(cells, cols, x + 5, y + 1, &input, TEXT);
    }
    write(cells, cols, x + width.saturating_sub(1), y + 1, "│", border);
    let hint = super::chat::composer_hint(busy, usize::from(width));
    let dash = usize::from(width).saturating_sub(hint.chars().count() + 2);
    write(
        cells,
        cols,
        x,
        y + 2,
        &format!("╰{}", "─".repeat(dash)),
        border,
    );
    write(cells, cols, x + 1 + dash as u16, y + 2, hint, DIM);
    write(cells, cols, x + width.saturating_sub(1), y + 2, "╯", border);
}

fn write(cells: &mut [Cell], cols: u16, x: u16, y: u16, text: &str, color: Rgba8) {
    let mut col = x;
    for ch in text.chars() {
        set(
            cells,
            cols,
            col,
            y,
            Cell {
                ch,
                fg: color,
                bg: BASE,
                bold: false,
                italic: false,
            },
        );
        col = col.saturating_add(UnicodeWidthChar::width(ch).unwrap_or(0) as u16);
    }
}

fn blend(from: Rgba8, to: Rgba8, amount: f32) -> Rgba8 {
    let channel = |a: u8, b: u8| {
        (i16::from(a) + ((i16::from(b) - i16::from(a)) as f32 * amount).round() as i16)
            .clamp(0, 255) as u8
    };
    rgb(
        channel(from.r, to.r),
        channel(from.g, to.g),
        channel(from.b, to.b),
    )
}

fn scene_row(cells: &[Cell]) -> NodeBuilder {
    let mut row = container().layout(
        LayoutStyle::flex_row()
            .height(metrics().h)
            .fill_width()
            .gap(0.0),
    );
    let mut start = 0;
    while start < cells.len() {
        let cell = cells[start];
        let mut end = start + 1;
        while end < cells.len() && cells[end] == cell {
            end += 1;
        }
        let text: String = std::iter::repeat_n(cell.ch, end - start).collect();
        let surface = Style::new().bg(cell.bg);
        let ink = Style::new().text_color(ColorAtom::Rgba8(cell.fg));
        let class = if cell.bold {
            "font-mono font-bold"
        } else {
            "font-mono"
        };
        row = row.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .width((end - start) as f32 * metrics().w)
                        .height(metrics().h),
                )
                .style(surface)
                .child(label(text).class(class).style(ink)),
        );
        start = end;
    }
    row
}

fn space(cols: u16, rows: u16, ticks: u64) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); usize::from(cols) * usize::from(rows)];
    let drift = (ticks / 150) as u16;
    for y in 0..rows {
        for x in 0..cols {
            let n = hash(x.wrapping_add(drift), y);
            if !n.is_multiple_of(28) {
                continue;
            }
            let phase = (ticks.wrapping_add(u64::from((n >> 7) % 192))) % 192;
            if phase <= 8 {
                continue;
            }
            let bright = n.is_multiple_of(5) && (9..=28).contains(&phase);
            set(
                &mut cells,
                cols,
                x,
                y,
                Cell {
                    ch: if bright { '✦' } else { '·' },
                    fg: if bright { STAR_BRIGHT } else { STAR_DIM },
                    bg: BASE,
                    bold: false,
                    italic: false,
                },
            );
        }
    }
    let phase = ticks % 440;
    if phase < 120 {
        let progress = phase as f32 / 120.0;
        let head_x = progress * (f32::from(cols) + 16.0) - 8.0;
        let head_y = progress * f32::from(rows) * 0.4;
        for step in 0..6_u16 {
            let x = head_x - f32::from(step) * 2.0;
            let y = head_y - f32::from(step) * 0.5;
            if x < 0.0 || y < 0.0 || x >= f32::from(cols) || y >= f32::from(rows) {
                continue;
            }
            let (ch, fg) = match step {
                0 => ('✦', STAR_BRIGHT),
                1 => ('·', STAR_MID),
                _ => ('·', STAR_DIM),
            };
            set(
                &mut cells,
                cols,
                x as u16,
                y as u16,
                Cell {
                    ch,
                    fg,
                    bg: BASE,
                    bold: false,
                    italic: false,
                },
            );
        }
    }
    cells
}

fn fantasy(cols: u16, rows: u16, ticks: u64) -> Vec<Cell> {
    let virtual_rows = rows.saturating_mul(2);
    let mut pixels = vec![SKY[0]; usize::from(cols) * usize::from(virtual_rows)];
    for y in 0..virtual_rows {
        for x in 0..cols {
            paint(&mut pixels, cols, x, y, sky(x, y, virtual_rows));
        }
    }
    paint_stars(&mut pixels, cols, virtual_rows, ticks);
    paint_sun(&mut pixels, cols, virtual_rows);
    paint_clouds(&mut pixels, cols, virtual_rows);
    paint_ridge(&mut pixels, cols, virtual_rows, FAR_RIDGE, FAR, true);
    paint_forest(&mut pixels, cols, virtual_rows);
    paint_castle(&mut pixels, cols, virtual_rows);
    paint_ground(&mut pixels, cols, virtual_rows, ticks);
    let pine_height = (0.24 * f32::from(virtual_rows)) as u16;
    let pine_base = virtual_rows.saturating_sub(1);
    paint_pine(
        &mut pixels,
        cols,
        cols.saturating_mul(6) / 100,
        pine_base,
        pine_height,
    );
    paint_pine(
        &mut pixels,
        cols,
        cols.saturating_mul(94) / 100,
        pine_base,
        pine_height,
    );
    let mut cells = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        for x in 0..cols {
            cells.push(Cell {
                ch: '▀',
                fg: pixel(&pixels, cols, x, y * 2),
                bg: pixel(&pixels, cols, x, y * 2 + 1),
                bold: false,
                italic: false,
            });
        }
    }
    cells
}

fn paint_stars(pixels: &mut [Rgba8], cols: u16, rows: u16, ticks: u64) {
    for y in 0..rows.saturating_mul(2) / 5 {
        for x in 0..cols {
            let n = hash(x, y);
            let present = n.is_multiple_of(28)
                || hash(x.wrapping_add(500), y.wrapping_add(500)).is_multiple_of(40);
            if !present || (ticks / 6 + u64::from(n % 7)).is_multiple_of(9) {
                continue;
            }
            if n.is_multiple_of(6) {
                paint(pixels, cols, x, y, STAR_LIT);
                for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
                    put(
                        pixels,
                        cols,
                        i32::from(x) + dx,
                        i32::from(y) + dy,
                        FANTASY_STAR_DIM,
                    );
                }
            } else {
                paint(pixels, cols, x, y, FANTASY_STAR_DIM);
            }
        }
    }
}

fn sky(x: u16, y: u16, height: u16) -> Rgba8 {
    let mut band = 0;
    for (index, seam) in [0.34_f32, 0.50, 0.64, 0.78].iter().enumerate() {
        let row = seam * f32::from(height);
        if f32::from(y) >= row {
            band = index + 1;
        }
        if (f32::from(y) - row).abs() <= 2.0 && (x + y) % 2 == 1 {
            return if f32::from(y) < row {
                SKY[index + 1]
            } else {
                SKY[index]
            };
        }
    }
    SKY[band]
}

fn paint_sun(pixels: &mut [Rgba8], cols: u16, rows: u16) {
    let cx = (0.11 * f32::from(cols)) as i32;
    let cy = (0.55 * f32::from(rows)) as i32;
    let radius = (0.06 * f32::from(cols)).max(5.0);
    for dy in -14_i32..=14 {
        for dx in -14_i32..=14 {
            let distance = ((dx * dx + dy * dy) as f32).sqrt();
            let checker = (cx + dx + cy + dy) % 2 == 0;
            let color = if distance <= radius {
                SUN
            } else if distance <= radius + 2.0 {
                SUN_MID
            } else if distance <= radius + 4.0 {
                if checker { SUN_MID } else { SUN_EDGE }
            } else if distance <= radius + 7.0 && checker {
                SUN_EDGE
            } else {
                continue;
            };
            put(pixels, cols, cx + dx, cy + dy, color);
        }
    }
}

fn paint_clouds(pixels: &mut [Rgba8], cols: u16, rows: u16) {
    for (cx, cy, length) in [(0.04_f32, 0.50_f32, 0.14_f32), (0.70, 0.24, 0.15)] {
        let start = (cx * f32::from(cols)) as i32;
        let length = (length * f32::from(cols)).max(3.0) as i32;
        let top = (cy * f32::from(rows)) as i32;
        for offset in 0..length {
            let x = start + offset;
            let height =
                2 + i32::try_from(hash(x.max(0) as u16, top.max(0) as u16) % 2).unwrap_or(0);
            for dy in 0..height {
                put(
                    pixels,
                    cols,
                    x,
                    top + dy,
                    if dy == 0 { CLOUD_LIT } else { CLOUD },
                );
            }
        }
    }
}

fn ridge(points: &[(f32, f32)], x: u16, width: u16) -> f32 {
    let position = f32::from(x) / f32::from(width.max(1));
    let mut previous = points.first().copied().unwrap_or((0.0, 0.5));
    for point in points.iter().skip(1) {
        if position <= point.0 {
            let span = (point.0 - previous.0).max(0.001);
            let progress = ((position - previous.0) / span).clamp(0.0, 1.0);
            return previous.1 + (point.1 - previous.1) * progress;
        }
        previous = *point;
    }
    previous.1
}

fn paint_ridge(
    pixels: &mut [Rgba8],
    cols: u16,
    rows: u16,
    points: &[(f32, f32)],
    color: Rgba8,
    rim: bool,
) {
    let mut previous_top = u16::MAX;
    for x in 0..cols {
        let top = (ridge(points, x, cols) * f32::from(rows)) as u16;
        if top < rows {
            let top_color = if rim && top == previous_top {
                FAR_RIM
            } else {
                color
            };
            paint(pixels, cols, x, top, top_color);
        }
        previous_top = top;
        for y in top.saturating_add(1)..rows {
            paint(pixels, cols, x, y, color);
        }
    }
}

fn paint_forest(pixels: &mut [Rgba8], cols: u16, rows: u16) {
    for x in 0..cols {
        let jag = (hash(x / 2, 91) % 4) as u16;
        let top = ((ridge(NEAR_RIDGE, x, cols) * f32::from(rows)) as u16).saturating_sub(jag);
        for y in top..rows {
            paint(pixels, cols, x, y, NEAR);
        }
    }
}

fn paint_castle(pixels: &mut [Rgba8], cols: u16, rows: u16) {
    let cx = i32::from(cols) * 17 / 20;
    let base = (ridge(NEAR_RIDGE, cx.max(0) as u16, cols) * f32::from(rows)) as i32 + 2;
    let keep_top = base - 12;
    for y in keep_top..=base {
        for x in cx - 3..=cx + 3 {
            put(pixels, cols, x, y, CASTLE);
        }
    }
    for y in base - 7..=base {
        for x in cx - 6..=cx - 4 {
            put(pixels, cols, x, y, CASTLE);
        }
    }
    for (left, top) in [(-3, keep_top), (-6, base - 7)] {
        for dx in 0..=3 {
            if dx % 2 == 0 {
                put(pixels, cols, cx + left + dx, top - 1, CASTLE);
            }
        }
    }
    for dy in 2..=4 {
        put(pixels, cols, cx, keep_top - dy, CASTLE);
    }
    put(pixels, cols, cx + 1, keep_top - 4, PENNANT);
    put(pixels, cols, cx + 2, keep_top - 4, PENNANT);
    for y in [base - 4, base - 8] {
        put(pixels, cols, cx, y, WINDOW);
    }
    put(pixels, cols, cx - 5, base - 4, WINDOW);
}

fn paint_ground(pixels: &mut [Rgba8], cols: u16, rows: u16, ticks: u64) {
    let top = (0.84 * f32::from(rows)) as u16;
    for x in 0..cols {
        for y in top..rows {
            paint(pixels, cols, x, y, GROUND);
        }
        for y in rows.saturating_sub(6)..rows {
            paint(pixels, cols, x, y, GROUND_DARK);
        }
        for y in top..rows {
            let n = hash(x, y);
            if n.is_multiple_of(89) {
                paint(pixels, cols, x, y, FLOWER);
            } else if n.is_multiple_of(13) {
                paint(pixels, cols, x, y, GRASS);
            }
        }
    }
    let fly_span = ((0.20 * f32::from(rows)) as u32).max(1);
    for i in 0..12_u16 {
        let n = hash(i, 211);
        let x = (n % u32::from(cols.max(1))) as u16;
        let y = (0.72 * f32::from(rows)) as u16 + ((n >> 8) % fly_span) as u16;
        if (ticks / 5 + u64::from(n % 4)) % 4 < 2 {
            paint(pixels, cols, x, y, FLY);
        }
    }
}

fn paint_pine(pixels: &mut [Rgba8], cols: u16, center: u16, base: u16, height: u16) {
    for step in 0..height {
        let half = i32::from((step / 2).min(4));
        let y = i32::from(base) - i32::from(step);
        for dx in -half..=half {
            put(pixels, cols, i32::from(center) + dx, y, CASTLE);
        }
    }
}

fn hash(x: u16, y: u16) -> u32 {
    let n = u32::from(x)
        .wrapping_mul(3_747_613_937)
        .wrapping_add(u32::from(y).wrapping_mul(2_654_435_761));
    n.wrapping_mul(4_294_967_291).wrapping_add(n >> 13)
}

fn paint(pixels: &mut [Rgba8], cols: u16, x: u16, y: u16, color: Rgba8) {
    let index = usize::from(y) * usize::from(cols) + usize::from(x);
    if let Some(pixel) = pixels.get_mut(index) {
        *pixel = color;
    }
}

fn put(pixels: &mut [Rgba8], cols: u16, x: i32, y: i32, color: Rgba8) {
    if let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) {
        paint(pixels, cols, x, y, color);
    }
}

fn pixel(pixels: &[Rgba8], cols: u16, x: u16, y: u16) -> Rgba8 {
    pixels
        .get(usize::from(y) * usize::from(cols) + usize::from(x))
        .copied()
        .unwrap_or(SPACE)
}

fn set(cells: &mut [Cell], cols: u16, x: u16, y: u16, cell: Cell) {
    let index = usize::from(y) * usize::from(cols) + usize::from(x);
    if let Some(slot) = cells.get_mut(index) {
        *slot = cell;
    }
}
