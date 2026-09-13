use std::ops::Range;
use std::time::Instant;

use sigil::prelude::*;
use sigil_tui::cell_width;
use unicode_segmentation::UnicodeSegmentation;

use super::app::App;
use super::canvas::{self, BASE, CYAN, DIM, GRAY, GREEN, TEXT};
use super::model::{Entry, Model};
use super::speed::Speed;
use crate::tui::{Activity, Round, context, fmt_tokens, short_session};

#[path = "chat/transcript.rs"]
pub(crate) mod transcript;

pub(super) use canvas::{Cell, rgb};
pub(super) const YELLOW: Rgba8 = rgb(220, 200, 90);
pub(super) const RED: Rgba8 = rgb(220, 90, 90);
pub(super) const BLACK: Rgba8 = rgb(0, 0, 0);
const USER_BG: Rgba8 = rgb(42, 44, 58);
const CODE_BG: Rgba8 = rgb(30, 32, 42);
const CODE_COMMENT: Rgba8 = rgb(96, 148, 102);
const CODE_STRING: Rgba8 = rgb(226, 178, 106);
const CODE_KEYWORD: Rgba8 = rgb(198, 146, 230);
const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const INPUT_ROWS: u16 = 8;
pub(super) const INPUT_HEIGHTS: [[StyleAtom; 1]; INPUT_ROWS as usize] = {
    let mut heights = [[StyleAtom::Height(StyleDimension::Auto)]; INPUT_ROWS as usize];
    let mut index = 0;
    while index < heights.len() {
        heights[index] = [StyleAtom::Height(StyleDimension::Px(Px(
            ((index + 3) as f32 * sigil_tui::Host::CELL) as i16,
        )))];
        index += 1;
    }
    heights
};
pub(super) const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Clone)]
pub(super) struct Span {
    text: String,
    fg: Rgba8,
    bg: Rgba8,
    bold: bool,
    italic: bool,
    source: Option<Source>,
}

#[derive(Clone)]
enum Source {
    Text(Range<usize>),
    Block(Range<usize>),
}

impl Source {
    fn range(&self) -> &Range<usize> {
        match self {
            Self::Text(range) | Self::Block(range) => range,
        }
    }
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
            source: None,
        }
    }

    fn surface(text: impl Into<String>, fg: Rgba8, bg: Rgba8) -> Self {
        Self {
            text: text.into(),
            fg,
            bg,
            bold: false,
            italic: false,
            source: None,
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
        cell_width(self.text.as_str())
    }

    fn source(mut self, start: usize) -> Self {
        self.source = Some(Source::Text(start..start + self.text.len()));
        self
    }

    #[cfg(test)]
    pub(crate) fn parts(&self) -> (&str, Rgba8) {
        (&self.text, self.fg)
    }
}

type Line = Vec<Span>;

pub(super) struct Placed {
    line: usize,
    cols: usize,
    rows: usize,
    source: ImageSource,
}

impl Placed {
    #[cfg(test)]
    pub(super) const fn line(&self) -> usize {
        self.line
    }

    #[cfg(test)]
    pub(super) const fn size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }
}

#[derive(Default)]
pub(super) struct History {
    key: Option<(u64, usize)>,
    lines: Vec<Line>,
    offsets: Vec<usize>,
    images: Vec<Placed>,
}

impl History {
    pub(super) fn extent(&self, rows: u16, draft: &str) -> Option<f32> {
        self.key.map(|_| {
            let height = usize::from(prompt_row(rows, draft).saturating_sub(1));
            self.lines.len().saturating_sub(height) as f32 * canvas::cell().h
        })
    }

    fn lines(&mut self, model: &Model, width: usize, revision: u64) -> &[Line] {
        if self.key != Some((revision, width)) {
            self.lines.clear();
            self.offsets.clear();
            self.images.clear();
            let rendered = transcript::entries(model, width, accent(&model.prefs.theme));
            for (entry, lines) in model.entries.iter().zip(rendered) {
                self.offsets.push(self.lines.len());
                self.lines.extend(wrap(lines, width));
                if let Entry::Image { source, .. } = entry {
                    let (cols, rows) = transcript::image_cells(source, width);
                    self.images.push(Placed {
                        line: self.lines.len().saturating_sub(rows),
                        cols,
                        rows,
                        source: source.clone(),
                    });
                }
            }
            self.key = Some((revision, width));
        }
        &self.lines
    }

    #[cfg(test)]
    pub(super) fn placements(&self) -> &[Placed] {
        &self.images
    }

    pub(super) fn position(&mut self, app: &App, entry: usize) -> Result<Option<f32>, String> {
        let width = usize::from(app.chat_cols());
        let left = if width > 28 {
            2
        } else {
            usize::from(width > 2)
        };
        let width = width.saturating_sub(left * 2).max(1);
        self.lines(&app.model.borrow(), width, app.panes.peek());
        let Some(&start) = self.offsets.get(entry) else {
            return Ok(None);
        };
        let end = self
            .offsets
            .get(entry + 1)
            .copied()
            .unwrap_or(self.lines.len());
        let lines = &self.lines[start..end];
        let pattern = app.search.pattern()?;
        if let Some(Entry::Assistant { text, .. }) = app.model.borrow().entries.get(entry)
            && let Some(found) = pattern.find(text)
        {
            let row = lines
                .iter()
                .position(|line| {
                    line.iter().any(|span| {
                        span.source.as_ref().is_some_and(|source| {
                            source.range().end > found.start() && source.range().start < found.end()
                        })
                    })
                })
                .or_else(|| {
                    lines.iter().position(|line| {
                        line.iter().any(|span| {
                            span.source
                                .as_ref()
                                .is_some_and(|source| source.range().start >= found.start())
                        })
                    })
                })
                .unwrap_or_else(|| lines.len().saturating_sub(1));
            return Ok(Some((start + row) as f32 * canvas::cell().h));
        }
        let text: String = lines
            .iter()
            .flatten()
            .map(|span| span.text.as_str())
            .collect();
        let row = pattern.find(&text).map_or(0, |found| {
            let mut bytes = 0;
            lines
                .iter()
                .position(|line| {
                    bytes += line.iter().map(|span| span.text.len()).sum::<usize>();
                    bytes > found.start()
                })
                .unwrap_or(0)
        });
        Ok(Some((start + row) as f32 * canvas::cell().h))
    }
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let press = app.clone();
    let drag = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .position(Position::Absolute)
                .inset(0.0)
                .fill_width()
                .fill_height()
                .gap(0.0),
        )
        .pointer_down(move |event| {
            if event.button == Some(MouseButton::Left)
                && let Some(byte) = composer_offset(&press, event.position)
            {
                press
                    .editor
                    .select(byte, event.modifiers.contains(ModifiersState::SHIFT));
                event.capture_pointer();
                event.stop_propagation();
            }
        })
        .pointer_move(move |event| {
            if event.is_captured()
                && let Some(byte) = composer_offset(&drag, event.position)
            {
                drag.editor.select(byte, true);
                event.stop_propagation();
            }
        })
        .children_reactive(move || {
            super::view::watch(&app);
            let (_, rows) = app.viewport.get();
            let cols = app.chat_cols();
            let draft = app.draft.get();
            let mut cells = grid(&app.model.borrow(), &app, &draft, cols, rows);
            if let Some(menu) = super::suggest::menu(&app, cols, rows) {
                stamp_menu(&mut cells, usize::from(cols).max(1), &menu);
            }
            let mut nodes = cells
                .chunks(usize::from(cols).max(1))
                .map(row)
                .collect::<Vec<_>>();
            nodes.extend(pictures(&app.model.borrow(), &app, cols, rows));
            nodes
        })
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
        &header(model, content_width, accent),
    );
    if height < 5 {
        return cells;
    }
    let prompt = usize::from(prompt_row(rows, draft));
    let busy = busy_status(model);
    let body_height = prompt.saturating_sub(1 + usize::from(busy.is_some()));
    let mut history = app.history.borrow_mut();
    let visible = history.lines(model, content_width, app.panes.get());
    let max_start = visible.len().saturating_sub(body_height);
    let start = ((app.scroll_y.peek() / canvas::cell().h) as usize).min(max_start);
    if visible.is_empty() && body_height > 0 {
        let hint = empty_hint(content_width);
        let pad = content_width.saturating_sub(cell_width(hint)) / 2;
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            1,
            &[Span::text(" ".repeat(pad)), Span::styled(hint, DIM)],
        );
    }
    for (offset, line) in visible.iter().skip(start).take(body_height).enumerate() {
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            1 + offset,
            line,
        );
    }
    if let Some(status) = &busy {
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            prompt.saturating_sub(1),
            &busy_line(status, content_width, accent),
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
        app.select.peek(),
        app.editor.selection(),
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

fn pictures(model: &Model, app: &App, cols: u16, rows: u16) -> Vec<NodeBuilder> {
    let width = usize::from(cols);
    if width == 0 || usize::from(rows) < 5 {
        return Vec::new();
    }
    let content_left = if width > 28 {
        2
    } else {
        usize::from(width > 2)
    };
    let prompt = usize::from(prompt_row(rows, &app.draft.peek()));
    let body_height = prompt.saturating_sub(1 + usize::from(busy_status(model).is_some()));
    let history = app.history.borrow();
    let max_start = history.lines.len().saturating_sub(body_height);
    let start = ((app.scroll_y.peek() / canvas::cell().h) as usize).min(max_start);
    let cell = canvas::cell();
    history
        .images
        .iter()
        .filter_map(|placed| {
            let top = placed.line;
            let first = top.max(start);
            let last = top
                .saturating_add(placed.rows)
                .min(start.saturating_add(body_height));
            if placed.rows == 0 || last <= first {
                return None;
            }
            let hidden = first - top;
            let shown = last - first;
            let pixels = placed.source.height() as usize;
            let mut picture = image(placed.source.clone())
                .fit(ObjectFit::Fill)
                .layout(LayoutStyle::block().fill_width().fill_height());
            if shown < placed.rows {
                let crop_top = hidden * pixels / placed.rows;
                let crop_height = (shown * pixels / placed.rows).max(1);
                picture = picture.crop(
                    u32::try_from(crop_top).unwrap_or(u32::MAX),
                    u32::try_from(crop_height).unwrap_or(u32::MAX),
                );
            }
            Some(
                container()
                    .layout(
                        LayoutStyle::block()
                            .position(Position::Absolute)
                            .inset_left(content_left as f32 * cell.w)
                            .inset_top((1 + first - start) as f32 * cell.h)
                            .width(placed.cols as f32 * cell.w)
                            .height(shown as f32 * cell.h),
                    )
                    .child(picture),
            )
        })
        .collect()
}

fn empty_hint(width: usize) -> &'static str {
    [
        "ask anything · /auth setup · @file attaches · !cmd runs · /help",
        "ask anything · /auth · @file · /help",
        "ask anything · /auth",
        "ask anything",
    ]
    .into_iter()
    .find(|hint| cell_width(hint) <= width)
    .unwrap_or("")
}

pub(super) fn input_rows(draft: &str, rows: u16) -> u16 {
    let limit = rows.saturating_sub(6).clamp(1, INPUT_ROWS);
    draft.split('\n').take(usize::from(limit)).count() as u16
}

pub(super) fn prompt_row(rows: u16, draft: &str) -> u16 {
    rows.saturating_sub(input_rows(draft, rows) + 3)
}

fn draft_rows(draft: &str, caret: usize, count: usize) -> (usize, usize) {
    let current = draft[..caret].bytes().filter(|byte| *byte == b'\n').count();
    let start = current
        .saturating_sub(count.saturating_sub(1))
        .min(draft.split('\n').count().saturating_sub(count));
    (current, start)
}

fn composer_offset(app: &App, point: Point) -> Option<usize> {
    if app.view_edit.peek() {
        return None;
    }
    let draft = app.draft.peek();
    let cols = usize::from(app.chat_cols());
    let rows = app.viewport.peek().1;
    let left = if cols > 28 { 2 } else { usize::from(cols > 2) };
    let col = (point.x / canvas::cell().w).max(0.0) as usize;
    let row = (point.y / canvas::cell().h).max(0.0) as usize;
    let top = usize::from(prompt_row(rows, &draft)) + 1;
    let count = usize::from(input_rows(&draft, rows));
    if !(top..top + count).contains(&row) || !(left..cols.saturating_sub(left)).contains(&col) {
        return None;
    }
    let caret = draft.floor_char_boundary(app.caret.peek().min(draft.len()));
    let (current, start) = draft_rows(&draft, caret, count);
    let index = start + row - top;
    let byte: usize = draft
        .split('\n')
        .take(index)
        .map(|line| line.len() + 1)
        .sum();
    let line = draft.split('\n').nth(index)?;
    let window = Draft::new(
        line,
        (index == current).then_some(caret.saturating_sub(byte)),
        cols.saturating_sub(left * 2 + 6),
    );
    Some(byte + window.offset(col.saturating_sub(left + 4)))
}

fn stamp_menu(cells: &mut [Cell], stride: usize, menu: &super::suggest::Menu) {
    for (offset, line) in menu.lines.iter().enumerate() {
        let y = menu.top.saturating_add(offset);
        let selected = line.chars().nth(1) == Some('›');
        let fg = if selected { TEXT } else { GRAY };
        let bg = if selected { USER_BG } else { CODE_BG };
        let mut span = Span::surface(line, fg, bg);
        span.bold = selected;
        write_line_at(
            cells,
            stride,
            menu.left,
            stride.saturating_sub(menu.left),
            y,
            &[span],
        );
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
    write_line_at(
        cells,
        width,
        left,
        width.saturating_sub(left),
        row,
        &[Span::styled(text, fg)],
    );
}

fn header(model: &Model, width: usize, accent: Rgba8) -> Line {
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
            format!("ses {} ", short_session(&model.session)),
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

#[expect(
    clippy::too_many_arguments,
    reason = "renders one prompt row from the current chat view state"
)]
fn prompt_rows(
    cells: &mut [Cell],
    area: [usize; 4],
    model: &Model,
    draft: &str,
    caret_position: usize,
    ticks: u64,
    editing: Option<&str>,
    selecting: bool,
    selection: Option<Range<usize>>,
) {
    let [stride, origin, width, top] = area;
    let height = cells.len().checked_div(stride).unwrap_or(0);
    let count = height.saturating_sub(top + 3).max(1);
    let position = draft.floor_char_boundary(caret_position.min(draft.len()));
    let (current, start) = draft_rows(draft, position, count);
    let total = draft.split('\n').count();
    let mode = if model.snapshot.plan { "plan" } else { "auto" };
    let mode_glyph = if model.snapshot.plan { '◇' } else { '●' };
    let mode_color = if model.snapshot.plan { CYAN } else { GREEN };
    let range = if total > count {
        format!(
            " · lines {}–{}/{total}",
            start + 1,
            (start + count).min(total)
        )
    } else {
        String::new()
    };
    let left = format!("╭ {mode_glyph} {mode}{range} ");
    let busy = model.busy_since.is_some();
    let border = if busy {
        accent(&model.prefs.theme)
    } else {
        GRAY
    };
    let model_width = width.saturating_sub(cell_width(left.as_str()) + 3);
    let shown_model = ellipsize(&model.model, model_width);
    let right = format!("{shown_model} ╮");
    let fill = width.saturating_sub(cell_width(left.as_str()) + cell_width(right.as_str()));
    write_line_at(
        cells,
        stride,
        origin,
        width,
        top,
        &[
            Span::styled("╭", border),
            Span::styled(format!(" {mode_glyph} {mode}{range} "), mode_color),
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
    let mut byte = 0;
    for (index, line) in draft.split('\n').enumerate() {
        let line_start = byte;
        let line_position = position.saturating_sub(byte).min(line.len());
        byte += line.len() + 1;
        if index < start {
            continue;
        }
        if index >= start + count {
            break;
        }
        let window = Draft::new(
            line,
            (index == current).then_some(line_position),
            interior.saturating_sub(2),
        );
        let (prompt, input) = match editing {
            Some(path) if index == start => (
                Span::text("  "),
                format!("editing {path} · ctrl+s save · esc leave"),
            ),
            Some(_) => (Span::text("  "), String::new()),
            None => (
                Span::styled(if index == current { "❯ " } else { "  " }, mode_color),
                window.text(caret),
            ),
        };
        let used = cell_width(input.as_str());
        write_line_at(
            cells,
            stride,
            origin,
            width,
            top + 1 + index - start,
            &[
                Span::styled("│", border),
                Span::text(" "),
                prompt,
                Span::text(input),
                Span::text(" ".repeat(interior.saturating_sub(used + 2))),
                Span::text(" "),
                Span::styled("│", border),
            ],
        );
        if editing.is_none()
            && let Some(selected) = &selection
        {
            let row = top + 1 + index - start;
            for col in 0..used {
                let offset = line_start + window.offset(col);
                if selected.contains(&offset) {
                    let cell = &mut cells[row * stride + origin + 4 + col];
                    cell.bg = USER_BG;
                }
            }
        }
    }
    let options = model.prefs.hotkeys.label(crate::prefs::Hotkey::Options);
    let select = selecting.then(|| model.prefs.hotkeys.label(crate::prefs::Hotkey::Select));
    let hint = composer_hint(
        model.busy_since.map(|(activity, _)| activity),
        width,
        &options,
        select.as_deref(),
    );
    let hint_width = cell_width(hint.as_str());
    let dash = width.saturating_sub(hint_width + 2);
    write_line_at(
        cells,
        stride,
        origin,
        width,
        top + 1 + count,
        &[
            Span::styled("╰", border),
            Span::styled("─".repeat(dash), border),
            Span::styled(hint, DIM),
            Span::styled("╯", border),
        ],
    );
}

pub(super) fn busy_status(model: &Model) -> Option<String> {
    model.busy_since.map(|(activity, since)| {
        let doing = match &model.live_tool {
            Some((name, arg)) => format!("{name} {arg}").trim_end().to_string(),
            None => activity.label().to_string(),
        };
        let (steering, queued) = model.queued;
        let waiting = match steering + queued {
            0 => String::new(),
            n => format!(" · {n} queued, ctrl+q edits"),
        };
        let rounds = if activity == Activity::Turn {
            format!(" · r{}", model.rounds.max(1))
        } else {
            String::new()
        };
        let now = Instant::now();
        let speed = model
            .speed
            .live(now)
            .or_else(|| model.speed.last())
            .map(|rate| format!(" · {}", Speed::label(rate)))
            .unwrap_or_default();
        format!(
            "{} {doing}{rounds} · {}s{speed}{waiting}",
            SPINNER[model.spinner],
            since.elapsed().as_secs()
        )
    })
}

fn busy_line(status: &str, width: usize, accent: Rgba8) -> Vec<Span> {
    let shown = ellipsize(status, width);
    let (spinner, rest) = shown.split_at(shown.find(' ').unwrap_or(shown.len()));
    vec![
        Span::styled(spinner.to_string(), accent),
        Span::styled(rest.to_string(), DIM),
    ]
}

pub(super) fn composer_hint(
    activity: Option<Activity>,
    width: usize,
    options: &str,
    select: Option<&str>,
) -> String {
    let available = width.saturating_sub(2);
    let choices = if let Some(key) = select {
        vec![format!(" select on · mouse to terminal · {key} off ")]
    } else if activity == Some(Activity::Branch) {
        vec![" branching · draft kept ".to_string()]
    } else if activity == Some(Activity::Turn) {
        vec![
            " esc cancel · enter steer · tab queue ".to_string(),
            " ↵ steer · tab queue · esc cancel ".to_string(),
            " ↵ steer · esc cancel ".to_string(),
        ]
    } else if activity.is_some() {
        vec![
            " esc cancel · enter/tab queue ".to_string(),
            " ↵ queue · esc cancel ".to_string(),
        ]
    } else {
        vec![
            format!(" enter send · {options} options "),
            format!(" ↵ send · {} options ", options.replace("ctrl+", "^")),
            " enter send ".to_string(),
        ]
    };
    choices
        .into_iter()
        .find(|hint| cell_width(hint.as_str()) <= available)
        .unwrap_or_default()
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
    let hidden_lines = line_start > 0 || line_end < draft.len();
    let prefix = if hidden_lines { "↵ " } else { "" };
    if cell_width(prefix) >= width {
        return ellipsize(prefix, width);
    }
    let window = Draft::new(
        &draft[line_start..line_end],
        Some(position - line_start),
        width - cell_width(prefix),
    );
    format!("{prefix}{}", window.text(caret))
}

struct Draft<'a> {
    before: &'a str,
    after: &'a str,
    start: usize,
    position: usize,
    left: bool,
    right: bool,
    cursor: bool,
}

impl<'a> Draft<'a> {
    fn new(text: &'a str, caret: Option<usize>, width: usize) -> Self {
        let position = text.floor_char_boundary(caret.unwrap_or(text.len()).min(text.len()));
        let cursor = caret.is_some() && width > 0;
        let available = width.saturating_sub(usize::from(cursor));
        if caret.is_none() {
            let right = cell_width(text) > available && available > 0;
            let end = prefix(text, available.saturating_sub(usize::from(right)));
            return Self {
                before: &text[..end],
                after: "",
                start: 0,
                position: end,
                left: false,
                right,
                cursor,
            };
        }
        let (before, after) = text.split_at(position);
        let budget = if before.is_empty() {
            available
        } else if after.is_empty() {
            0
        } else {
            available / 2
        };
        let right = cell_width(after) > budget && budget > 0;
        let end = prefix(after, budget.saturating_sub(usize::from(right)));
        let after = &after[..end];
        let budget = available.saturating_sub(cell_width(after) + usize::from(right));
        let left = cell_width(before) > budget && budget > 0;
        let mut used = 0;
        let mut start = position;
        for (index, grapheme) in before.grapheme_indices(true).rev() {
            used += cell_width(grapheme);
            if used > budget.saturating_sub(usize::from(left)) {
                break;
            }
            start = index;
        }
        Self {
            before: &text[start..position],
            after,
            start,
            position,
            left,
            right,
            cursor,
        }
    }

    fn text(&self, caret: char) -> String {
        let mut text = String::new();
        if self.left {
            text.push('…');
        }
        text.push_str(self.before);
        if self.cursor {
            text.push(caret);
        }
        text.push_str(self.after);
        if self.right {
            text.push('…');
        }
        text
    }

    fn offset(&self, col: usize) -> usize {
        let mut x = usize::from(self.left);
        for (index, (text, start)) in [(self.before, self.start), (self.after, self.position)]
            .into_iter()
            .enumerate()
        {
            if index == 1 {
                if self.cursor && col <= x {
                    return self.position;
                }
                x += usize::from(self.cursor);
            }
            for (byte, grapheme) in text.grapheme_indices(true) {
                let width = cell_width(grapheme);
                if col < x + width {
                    return start + byte;
                }
                x += width;
            }
        }
        self.position + self.after.len()
    }
}

fn prefix(text: &str, width: usize) -> usize {
    let mut used = 0;
    text.grapheme_indices(true)
        .find_map(|(byte, grapheme)| {
            used += cell_width(grapheme);
            (used > width).then_some(byte)
        })
        .unwrap_or(text.len())
}

pub(super) fn ellipsize(text: &str, width: usize) -> String {
    if cell_width(text) <= width {
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
    right.push(Span::styled(
        format!(
            "ctx {} · ",
            context(model.usage.last(), model.snapshot.context_window)
        ),
        DIM,
    ));
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
    if model.busy_since.is_none()
        && let Some(rate) = model.speed.last()
    {
        right.push(Span::styled(format!(" · {}", Speed::label(rate)), DIM));
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
        for mut span in line {
            let mut chunk = String::new();
            for grapheme in span.text.graphemes(true) {
                let step = cell_width(grapheme);
                if used + step > width && used > 0 {
                    let mut part = span.clone();
                    part.text = std::mem::take(&mut chunk);
                    if let Some(Source::Text(source)) = &mut part.source {
                        source.end = source.start + part.text.len();
                    }
                    if let Some(Source::Text(source)) = &mut span.source {
                        source.start += part.text.len();
                    }
                    if !part.text.is_empty() {
                        row.push(part);
                    }
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                chunk.push_str(grapheme);
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
    line: &[Span],
) {
    let start = y.saturating_mul(stride).saturating_add(left);
    let Some(row) = cells.get_mut(start..start.saturating_add(width)) else {
        return;
    };
    let mut x = 0;
    for span in line {
        for text in span.text.graphemes(true) {
            let step = cell_width(text);
            if x >= width || x.saturating_add(step) > width {
                return;
            }
            if step == 0 {
                continue;
            }
            row[x] = Cell {
                text: text.into(),
                fg: span.fg,
                bg: span.bg,
                bold: span.bold,
                italic: span.italic,
            };
            for pad in 1..step {
                if x + pad < width {
                    row[x + pad] = Cell {
                        text: "".into(),
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
    let end = text.grapheme_indices(true).find_map(|(index, grapheme)| {
        used += cell_width(grapheme);
        (used > width).then_some(index)
    });
    if let Some(end) = end {
        text.truncate(end);
    }
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
