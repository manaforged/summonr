use sigil::prelude::*;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::App;
use super::canvas::{self, BASE, CYAN, DIM, GRAY, GREEN, TEXT};
use super::model::Model;
use crate::tui::{Round, fmt_tokens, short_session};

#[path = "chat/transcript.rs"]
mod transcript;

#[path = "chat/image.rs"]
pub(super) mod image;

pub(super) use canvas::{Cell, rgb};
pub(super) const YELLOW: Rgba8 = rgb(220, 200, 90);
pub(super) const RED: Rgba8 = rgb(220, 90, 90);
pub(super) const BLACK: Rgba8 = rgb(0, 0, 0);
const USER_BG: Rgba8 = rgb(42, 44, 58);
const CODE_BG: Rgba8 = rgb(30, 32, 42);
const CODE_COMMENT: Rgba8 = rgb(96, 148, 102);
const CODE_STRING: Rgba8 = rgb(226, 178, 106);
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
pub(super) const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Clone)]
struct Span {
    text: String,
    fg: Rgba8,
    bg: Rgba8,
    bold: bool,
    italic: bool,
}

impl Span {
    fn text(text: impl Into<String>) -> Self {
        Self::styled(text, TEXT)
    }

    fn styled(text: impl Into<String>, fg: Rgba8) -> Self {
        Self {
            text: text.into(),
            fg,
            bg: BASE,
            bold: false,
            italic: false,
        }
    }

    fn surface(text: impl Into<String>, fg: Rgba8, bg: Rgba8) -> Self {
        Self {
            text: text.into(),
            fg,
            bg,
            bold: false,
            italic: false,
        }
    }

    fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    fn width(&self) -> usize {
        UnicodeWidthStr::width(self.text.as_str())
    }
}

type Line = Vec<Span>;

pub(super) fn layer(app: App) -> NodeBuilder {
    let (_, rows) = app.viewport.get();
    let cols = app.chat_cols();
    let draft = app.draft.get();
    let mut cells = grid(&app.model.borrow(), &app, &draft, cols, rows);
    if let Some(menu) = super::suggest::menu(&app, cols, rows) {
        stamp_menu(&mut cells, usize::from(cols).max(1), &menu);
    }
    let mut layer = container().layout(
        LayoutStyle::flex_column()
            .position(Position::Absolute)
            .inset(0.0)
            .fill_width()
            .fill_height()
            .gap(0.0),
    );
    for cells in cells.chunks(usize::from(cols).max(1)) {
        layer = layer.child(row(cells));
    }
    layer
}

fn grid(model: &Model, app: &App, draft: &str, cols: u16, rows: u16) -> Vec<Cell> {
    let width = usize::from(cols);
    let height = usize::from(rows);
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    if width == 0 || height == 0 {
        return cells;
    }
    let content_left = if width > 28 {
        2
    } else {
        usize::from(width > 2)
    };
    let content_width = width.saturating_sub(content_left * 2);
    let accent = accent(&model.prefs.theme);
    write_line_at(
        &mut cells,
        width,
        content_left,
        content_width,
        0,
        &header(model, app, content_width, accent),
    );
    if height < 5 {
        return cells;
    }
    let prompt = usize::from(prompt_row(rows));
    let body_height = prompt.saturating_sub(1);
    let transcript = transcript::entries(model, content_width, accent);
    let visible = wrap(transcript, content_width);
    let max_start = visible.len().saturating_sub(body_height);
    let start = ((app.scroll_y.peek() / canvas::cell().h) as usize).min(max_start);
    if visible.is_empty() && body_height > 0 {
        let hint = empty_hint(content_width);
        let pad = content_width.saturating_sub(UnicodeWidthStr::width(hint)) / 2;
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            1,
            &vec![Span::text(" ".repeat(pad)), Span::styled(hint, DIM)],
        );
    }
    for (offset, line) in visible
        .into_iter()
        .skip(start)
        .take(body_height)
        .enumerate()
    {
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            1 + offset,
            &line,
        );
    }
    let editing = app
        .view_edit
        .peek()
        .then(|| model.view.as_ref().map(|view| view.path.clone()))
        .flatten();
    prompt_rows(
        &mut cells,
        [width, content_left, content_width, prompt],
        model,
        draft,
        app.caret.peek(),
        app.ticks.peek(),
        editing.as_deref(),
    );
    write_line_at(
        &mut cells,
        width,
        content_left,
        content_width,
        height - 1,
        &status(model, app, content_width, accent),
    );
    cells
}

fn empty_hint(width: usize) -> &'static str {
    [
        "ask anything — @file attaches · !cmd runs · /help lists",
        "ask anything · @file · !cmd · /help",
        "ask anything · /help",
        "ask anything",
    ]
    .into_iter()
    .find(|hint| UnicodeWidthStr::width(*hint) <= width)
    .unwrap_or("")
}

pub(super) const fn prompt_row(rows: u16) -> u16 {
    rows.saturating_sub(4)
}

fn stamp_menu(cells: &mut [Cell], stride: usize, menu: &super::suggest::Menu) {
    for (offset, line) in menu.lines.iter().enumerate() {
        let y = menu.top.saturating_add(offset);
        let selected = line.chars().nth(1) == Some('›');
        let fg = if selected { TEXT } else { GRAY };
        let bg = if selected { USER_BG } else { CODE_BG };
        let mut col = menu.left;
        for ch in line.chars() {
            if col >= stride {
                break;
            }
            if let Some(cell) = cells.get_mut(y.saturating_mul(stride).saturating_add(col)) {
                cell.ch = ch;
                cell.fg = fg;
                cell.bg = bg;
                cell.bold = selected;
                cell.italic = false;
            }
            col = col.saturating_add(UnicodeWidthChar::width(ch).unwrap_or(0));
        }
    }
}

pub(super) fn write_at(
    cells: &mut [Cell],
    width: usize,
    left: usize,
    row: usize,
    text: &str,
    fg: Rgba8,
) {
    let mut col = left;
    for ch in text.chars() {
        if col >= width {
            return;
        }
        if let Some(cell) = cells.get_mut(row * width + col) {
            cell.ch = ch;
            cell.fg = fg;
            cell.bg = BASE;
            cell.bold = false;
            cell.italic = false;
        }
        col += UnicodeWidthChar::width(ch).unwrap_or(0);
    }
}

fn header(model: &Model, app: &App, width: usize, accent: Rgba8) -> Line {
    let mut left = vec![
        Span::styled("summoner ", accent).bold(),
        Span::text(model.model.clone()),
    ];
    if width >= 80
        && let Some(relay) = model.relay.as_deref().filter(|relay| !relay.is_empty())
    {
        left.push(Span::styled(format!(" · {relay}"), DIM));
    }
    let mode = if model.snapshot.plan { "plan" } else { "auto" };
    let mode_color = if model.snapshot.plan { CYAN } else { GREEN };
    let mut right = Vec::new();
    if width >= 70 {
        right.push(Span::styled(
            format!("ses {} ", short_session(&app.info.session)),
            DIM,
        ));
    }
    right.push(Span::styled(format!("{mode} "), mode_color));
    if width >= 70 && model.snapshot.strict {
        right.push(Span::styled("strict ", YELLOW));
    }
    if width >= 48 {
        right.push(Span::styled(
            format!(
                "{} tok ",
                fmt_tokens(model.prompt_tokens + model.completion_tokens)
            ),
            DIM,
        ));
    }
    join(left, right, width)
}

fn prompt_rows(
    cells: &mut [Cell],
    area: [usize; 4],
    model: &Model,
    draft: &str,
    caret_position: usize,
    ticks: u64,
    editing: Option<&str>,
) {
    let [stride, origin, width, top] = area;
    let mode = if model.snapshot.plan { "plan" } else { "auto" };
    let mode_glyph = if model.snapshot.plan { '◇' } else { '●' };
    let mode_color = if model.snapshot.plan { CYAN } else { GREEN };
    let left = format!("╭ {mode_glyph} {mode} ");
    let busy = model.busy_since.is_some();
    let border = if busy {
        accent(&model.prefs.theme)
    } else {
        GRAY
    };
    let model_width = width.saturating_sub(UnicodeWidthStr::width(left.as_str()) + 3);
    let shown_model = ellipsize(&model.model, model_width);
    let right = format!("{shown_model} ╮");
    let fill = width.saturating_sub(
        UnicodeWidthStr::width(left.as_str()) + UnicodeWidthStr::width(right.as_str()),
    );
    write_line_at(
        cells,
        stride,
        origin,
        width,
        top,
        &vec![
            Span::styled("╭", border),
            Span::styled(format!(" {mode_glyph} {mode} "), mode_color),
            Span::styled("─".repeat(fill), border),
            Span::styled(shown_model, DIM),
            Span::text(" "),
            Span::styled("╮", border),
        ],
    );
    let interior = width.saturating_sub(4);
    let caret = if (ticks / 6).is_multiple_of(2) {
        '▌'
    } else {
        ' '
    };
    let (prompt, input) = match editing {
        Some(path) => (
            Span::text("  "),
            format!("editing {path} · ctrl+s save · esc leave"),
        ),
        None => (
            Span::styled("❯ ", mode_color),
            draft_view(draft, caret_position, interior.saturating_sub(2), caret),
        ),
    };
    let used = UnicodeWidthStr::width(input.as_str());
    write_line_at(
        cells,
        stride,
        origin,
        width,
        top + 1,
        &vec![
            Span::styled("│", border),
            Span::text(" "),
            prompt,
            Span::text(input),
            Span::text(" ".repeat(interior.saturating_sub(used + 2))),
            Span::text(" "),
            Span::styled("│", border),
        ],
    );
    let hint = composer_hint(busy, width);
    let hint_width = UnicodeWidthStr::width(hint);
    let dash = width.saturating_sub(hint_width + 2);
    write_line_at(
        cells,
        stride,
        origin,
        width,
        top + 2,
        &vec![
            Span::styled("╰", border),
            Span::styled("─".repeat(dash), border),
            Span::styled(hint, DIM),
            Span::styled("╯", border),
        ],
    );
}

pub(super) fn composer_hint(busy: bool, width: usize) -> &'static str {
    let available = width.saturating_sub(2);
    let choices: &[&str] = if busy {
        &[
            " esc cancel · enter steer · tab queue ",
            " ↵ steer · tab queue · esc cancel ",
            " ↵ steer · esc cancel ",
        ]
    } else {
        &[
            " enter send · ctrl+o settings · ctrl+j newline · shift+tab mode · ctrl+e files · ctrl+x diff · ctrl+p pane ",
            " enter send · ctrl+o settings · ctrl+j newline · shift+tab mode ",
            " enter send · ^j newline · ⇧tab mode ",
            " enter send · ^j newline ",
            " enter send ",
        ]
    };
    choices
        .iter()
        .copied()
        .find(|hint| UnicodeWidthStr::width(*hint) <= available)
        .unwrap_or("")
}

pub(super) fn draft_view(draft: &str, position: usize, width: usize, caret: char) -> String {
    if width == 0 {
        return String::new();
    }
    let mut position = position.min(draft.len());
    while position > 0 && !draft.is_char_boundary(position) {
        position -= 1;
    }
    let line_start = draft[..position].rfind('\n').map_or(0, |index| index + 1);
    let line_end = draft[position..]
        .find('\n')
        .map_or(draft.len(), |index| position + index);
    let before = &draft[line_start..position];
    let after = &draft[position..line_end];
    let hidden_lines = line_start > 0 || line_end < draft.len();
    let prefix = if hidden_lines { "↵ " } else { "" };
    let available = width
        .saturating_sub(UnicodeWidthStr::width(prefix))
        .saturating_sub(UnicodeWidthChar::width(caret).unwrap_or(0));
    let before_width = UnicodeWidthStr::width(before);
    let after_width = UnicodeWidthStr::width(after);
    let after_budget = if before_width == 0 {
        available
    } else if after_width == 0 {
        0
    } else {
        available / 2
    };
    let shown_after = head(after, after_budget);
    let shown_before = tail(
        before,
        available.saturating_sub(UnicodeWidthStr::width(shown_after.as_str())),
    );
    format!("{prefix}{shown_before}{caret}{shown_after}")
}

fn head(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut shown = text.to_string();
    truncate(&mut shown, width.saturating_sub(1));
    shown.push('…');
    shown
}

fn tail(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let available = width.saturating_sub(1);
    let mut used = 0;
    let mut chars = Vec::new();
    for ch in text.chars().rev() {
        let step = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + step > available {
            break;
        }
        chars.push(ch);
        used += step;
    }
    chars.reverse();
    format!("…{}", chars.into_iter().collect::<String>())
}

pub(super) fn ellipsize(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut shown = text.to_string();
    truncate(&mut shown, width.saturating_sub(1));
    shown.push('…');
    shown
}

fn status(model: &Model, app: &App, width: usize, accent: Rgba8) -> Line {
    let mut left = vec![Span::styled(format!("📂 {}", app.info.workspace), DIM)];
    if let Some(branch) = &app.git_branch {
        left.push(Span::styled("  ⎇ ", DIM));
        left.push(Span::styled(branch, YELLOW));
    }
    if model.peers > 0 {
        left.push(Span::styled(
            format!(
                "  · {} agent{} live",
                model.peers,
                if model.peers == 1 { "" } else { "s" }
            ),
            accent,
        ));
    }
    let mut right = Vec::new();
    if let Some(since) = model.busy_since {
        right.push(Span::styled(
            format!("{} working", SPINNER[model.spinner]),
            accent,
        ));
        right.push(Span::styled(
            format!(
                " · r{} · {}s",
                model.rounds.max(1),
                since.elapsed().as_secs()
            ),
            DIM,
        ));
        if let Some((name, arg)) = &model.live_tool {
            right.push(Span::styled(format!(" · {name} {arg}"), DIM));
        }
        return join(left, right, width);
    }
    if let Some(window) = model.snapshot.context_window
        && let Some(last) = model.usage.last()
        && window > 0
    {
        let used = last.total().min(window);
        right.push(Span::styled(
            format!(
                "ctx {}% · ",
                100 - u64::from(used) * 100 / u64::from(window)
            ),
            DIM,
        ));
    }
    let spark = sparkline(&model.usage);
    if !spark.is_empty() {
        right.push(Span::styled(format!("{spark} "), accent));
    }
    right.push(Span::styled(
        format!(
            "⚡ {} tok",
            fmt_tokens(model.prompt_tokens + model.completion_tokens)
        ),
        DIM,
    ));
    if model.cached_tokens > 0 && model.prompt_tokens > 0 {
        right.push(Span::styled(
            format!(
                " · {}% cached",
                model.cached_tokens * 100 / model.prompt_tokens
            ),
            DIM,
        ));
    }
    join(left, right, width)
}

fn sparkline(rounds: &[Round]) -> String {
    if rounds.len() < 2 {
        return String::new();
    }
    let peak = rounds
        .iter()
        .map(|round| round.total())
        .max()
        .unwrap_or(0)
        .max(1) as usize;
    rounds
        .iter()
        .rev()
        .take(12)
        .rev()
        .map(|round| SPARK[round.total() as usize * 7 / peak])
        .collect()
}

fn join(mut left: Line, right: Line, width: usize) -> Line {
    let right = clip_line(right, width);
    let right_width = line_width(&right);
    let gap = usize::from(right_width < width);
    left = clip_line(left, width.saturating_sub(right_width + gap));
    let used = line_width(&left) + right_width;
    left.push(Span::text(" ".repeat(width.saturating_sub(used))));
    left.extend(right);
    left
}

fn clip_line(mut line: Line, width: usize) -> Line {
    let mut used = 0;
    for span in &mut line {
        truncate(&mut span.text, width.saturating_sub(used));
        used += span.width();
    }
    line.retain(|span| !span.text.is_empty());
    line
}

fn pad(mut line: Line, width: usize, bg: Rgba8) -> Line {
    let used = line_width(&line);
    if used < width {
        line.push(Span::surface(" ".repeat(width - used), TEXT, bg));
    }
    line
}

fn line_width(line: &Line) -> usize {
    line.iter().map(Span::width).sum()
}

fn wrap(lines: Vec<Line>, width: usize) -> Vec<Line> {
    let mut rows = Vec::new();
    for line in lines {
        if line_width(&line) <= width {
            rows.push(line);
            continue;
        }
        let mut row = Vec::new();
        let mut used = 0;
        for span in line {
            let mut chunk = String::new();
            for ch in span.text.chars() {
                let step = UnicodeWidthChar::width(ch).unwrap_or(0);
                if used + step > width && !chunk.is_empty() {
                    let mut part = span.clone();
                    part.text = std::mem::take(&mut chunk);
                    row.push(part);
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                chunk.push(ch);
                used += step;
            }
            if !chunk.is_empty() {
                let mut part = span;
                part.text = chunk;
                row.push(part);
            }
        }
        rows.push(row);
    }
    rows
}

fn write_line_at(
    cells: &mut [Cell],
    stride: usize,
    left: usize,
    width: usize,
    y: usize,
    line: &Line,
) {
    let start = y.saturating_mul(stride).saturating_add(left);
    let Some(row) = cells.get_mut(start..start.saturating_add(width)) else {
        return;
    };
    let mut x = 0;
    for span in line {
        for ch in span.text.chars() {
            let step = UnicodeWidthChar::width(ch).unwrap_or(0);
            if x >= width {
                return;
            }
            row[x] = Cell {
                ch,
                fg: span.fg,
                bg: span.bg,
                bold: span.bold,
                italic: span.italic,
            };
            for pad in 1..step {
                if x + pad < width {
                    row[x + pad] = Cell {
                        ch: '\0',
                        fg: span.fg,
                        bg: span.bg,
                        bold: span.bold,
                        italic: span.italic,
                    };
                }
            }
            x += step;
        }
    }
}

pub(super) fn row(cells: &[Cell]) -> NodeBuilder {
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .height(canvas::cell().h)
                .fill_width()
                .gap(0.0),
        )
        .style(Style::new().bg(BASE));
    let mut start = 0;
    while start < cells.len() {
        let cell = cells[start];
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
            .filter_map(|cell| (cell.ch != '\0').then_some(cell.ch))
            .collect();
        let metrics = canvas::cell();
        let class = match (cell.bold, cell.italic, metrics.text.is_empty()) {
            (true, true, true) => "font-mono font-bold italic",
            (true, false, true) => "font-mono font-bold",
            (false, true, true) => "font-mono italic",
            (false, false, true) => "font-mono",
            (true, true, false) => "font-mono font-bold italic text-sm",
            (true, false, false) => "font-mono font-bold text-sm",
            (false, true, false) => "font-mono italic text-sm",
            (false, false, false) => "font-mono text-sm",
        };
        row = row.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .width((end - start) as f32 * metrics.w)
                        .height(metrics.h),
                )
                .style(Style::new().bg(cell.bg))
                .child(
                    label(text)
                        .class(class)
                        .style(Style::new().text_color(ColorAtom::Rgba8(cell.fg))),
                ),
        );
        start = end;
    }
    row
}

fn truncate(text: &mut String, width: usize) {
    let mut used = 0;
    text.retain(|ch| {
        let step = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + step > width {
            false
        } else {
            used += step;
            true
        }
    });
}

pub(super) fn accent(name: &str) -> Rgba8 {
    if name == "fantasy" {
        rgb(255, 208, 96)
    } else {
        rgb(105, 140, 210)
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
