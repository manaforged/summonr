use std::iter::once;
use std::ops::Range;
use std::path::Path;
use std::time::Instant;

use sigil::prelude::*;
use sigil_tui::cell_width;
use unicode_segmentation::UnicodeSegmentation;

use super::app::App;
use super::canvas::{self, palette};
use super::input::{self, Caret, Input};
use super::model::{Entry, Model};
use super::speed::Speed;
use crate::tui::{Activity, Round, context, fmt_tokens, short_session};

#[path = "chat/transcript.rs"]
pub(crate) mod transcript;

pub(super) use canvas::{Cell, rgb};
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
        Self::styled(text, palette().text)
    }

    fn styled(text: impl Into<String>, fg: Rgba8) -> Self {
        Self {
            text: text.into(),
            fg,
            bg: palette().base,
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

    fn slice(&self, range: Range<usize>) -> Self {
        let source = match &self.source {
            Some(Source::Text(source)) => Some(Source::Text(
                source.start + range.start..source.start + range.end,
            )),
            _ => self.source.clone(),
        };
        Self {
            text: self.text[range].to_string(),
            fg: self.fg,
            bg: self.bg,
            bold: self.bold,
            italic: self.italic,
            source,
        }
    }

    #[cfg(test)]
    pub(crate) fn parts(&self) -> (&str, Rgba8) {
        (&self.text, self.fg)
    }
}

struct Line {
    spans: Vec<Span>,
    bg: Rgba8,
}

impl From<Vec<Span>> for Line {
    fn from(spans: Vec<Span>) -> Self {
        Self {
            spans,
            bg: palette().base,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Anchor {
    entry: usize,
    line: usize,
    byte: usize,
}

pub(super) struct Row {
    spans: Vec<Span>,
    bg: Rgba8,
    anchor: Anchor,
}

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
    stamp: u64,
    width: usize,
    stable: usize,
    lines: Vec<Row>,
    images: Vec<Placed>,
    stream: Option<transcript::Stream>,
    #[cfg(test)]
    pub(super) rendered: usize,
}

impl History {
    pub(super) fn anchor(&self, position: f32) -> Option<Anchor> {
        self.lines
            .get((position / canvas::cell().h) as usize)
            .map(|row| row.anchor)
    }

    pub(super) fn locate(&self, anchor: Anchor) -> Option<f32> {
        let row = self
            .lines
            .partition_point(|row| row.anchor <= anchor)
            .saturating_sub(1);
        self.lines
            .get(row)
            .filter(|row| row.anchor.entry == anchor.entry)?;
        Some(row as f32 * canvas::cell().h)
    }

    pub(super) fn extent(
        &mut self,
        model: &Model,
        cols: u16,
        rows: u16,
        draft: &str,
        stamp: u64,
    ) -> f32 {
        let width = usize::from(cols);
        let inset = margin(width);
        let height = usize::from(body_rows(cols, rows, draft, model.busy_since.is_some()));
        self.lines(model, width.saturating_sub(inset * 2), stamp)
            .len()
            .saturating_sub(height) as f32
            * canvas::cell().h
    }

    pub(super) fn lines(&mut self, model: &Model, width: usize, stamp: u64) -> &[Row] {
        let len = model.entries.len();
        if let Some(Entry::Assistant { text, done: false }) =
            model.entries.get(len.saturating_sub(1))
            && self.width == width
            && self.stable == len
            && self
                .stream
                .as_ref()
                .is_some_and(|stream| stream.matches(text))
        {
            let index = len - 1;
            let accent = accent(&model.prefs.theme);
            let first = self.stream.as_ref().map_or(0, transcript::Stream::lines);
            let (committed, mut tail) = self
                .stream
                .as_mut()
                .expect("stream checked above")
                .extend(text, width, accent);
            let keep = self.lines.partition_point(|row| {
                row.anchor.entry < index || (row.anchor.entry == index && row.anchor.line < first)
            });
            self.lines.truncate(keep);
            self.images.retain(|placed| placed.line < keep);
            if committed.is_empty() && tail.is_empty() && self.lines.is_empty() {
                tail.push(Vec::new().into());
            }
            if let Some(last) = tail.last_mut() {
                last.spans.push(Span::surface("▌", palette().dim, last.bg));
            }
            let tail_first = first + committed.len();
            self.lines.extend(wrap(committed, width, index, first));
            self.lines.extend(wrap(tail, width, index, tail_first));
            self.stamp = stamp;
            self.width = width;
            self.stable = len;
            return &self.lines;
        }
        let start = if self.width != width || len < self.stable {
            0
        } else if len > self.stable || self.stamp != stamp {
            self.stable.saturating_sub(1)
        } else {
            self.stable
        };
        if start == 0 {
            self.lines.clear();
            self.images.clear();
        } else {
            let split = self.lines.partition_point(|row| row.anchor.entry < start);
            self.lines.truncate(split);
            self.images.retain(|placed| placed.line < split);
        }
        let accent = accent(&model.prefs.theme);
        let reasoning = model.prefs.reasoning;
        let tools = model.prefs.tools;
        let receipts = model.prefs.receipts;
        let mut previous_tool =
            start > 0 && matches!(&model.entries[start - 1], Entry::Tool { .. });
        for index in start..len {
            let entry = &model.entries[index];
            let separated = index > 0 && !(previous_tool && matches!(entry, Entry::Tool { .. }));
            let rendered = transcript::entry_lines(
                entry, width, accent, reasoning, tools, receipts, separated,
            );
            self.lines.extend(wrap(rendered, width, index, 0));
            if let Entry::Image { source, .. } = entry {
                let (cols, rows) = transcript::image_cells(source, width);
                self.images.push(Placed {
                    line: self.lines.len().saturating_sub(rows),
                    cols,
                    rows,
                    source: source.clone(),
                });
            }
            previous_tool = matches!(entry, Entry::Tool { .. });
        }
        self.stream = match model.entries.last() {
            Some(Entry::Assistant { text, done: false }) => {
                Some(transcript::Stream::new(text, width, accent))
            }
            _ => None,
        };
        #[cfg(test)]
        {
            self.rendered += len - start;
        }
        self.stamp = stamp;
        self.width = width;
        self.stable = len;
        &self.lines
    }

    #[cfg(test)]
    pub(super) fn placements(&self) -> &[Placed] {
        &self.images
    }

    #[cfg(test)]
    pub(super) fn fingerprint(&self) -> Vec<(usize, usize, usize, String)> {
        self.lines
            .iter()
            .map(|row| {
                (
                    row.anchor.entry,
                    row.anchor.line,
                    row.anchor.byte,
                    row.spans.iter().map(|span| span.text.as_str()).collect(),
                )
            })
            .collect()
    }

    #[cfg(test)]
    pub(super) fn last_entry(&self) -> Option<usize> {
        self.lines.last().map(|row| row.anchor.entry)
    }

    pub(super) fn position(&mut self, app: &App, entry: usize) -> Result<Option<f32>, String> {
        let width = usize::from(app.chat_cols());
        let left = margin(width);
        let width = width.saturating_sub(left * 2).max(1);
        let model = app.model.borrow();
        self.lines(&model, width, model.entries.stamp);
        let start = self.lines.partition_point(|row| row.anchor.entry < entry);
        let end = self.lines.partition_point(|row| row.anchor.entry <= entry);
        if start == end {
            return Ok(None);
        }
        let lines = &self.lines[start..end];
        let pattern = app.search.pattern()?;
        if let Some(Entry::Assistant { text, .. }) = model.entries.get(entry)
            && let Some(found) = pattern.find(text)
        {
            let row = lines
                .iter()
                .position(|line| {
                    line.spans.iter().any(|span| {
                        span.source.as_ref().is_some_and(|source| {
                            source.range().end > found.start() && source.range().start < found.end()
                        })
                    })
                })
                .or_else(|| {
                    lines.iter().position(|line| {
                        line.spans.iter().any(|span| {
                            span.source
                                .as_ref()
                                .is_some_and(|source| source.range().start >= found.start())
                        })
                    })
                })
                .unwrap_or_else(|| lines.len().saturating_sub(1));
            return Ok(Some((start + row) as f32 * canvas::cell().h));
        }
        let mut text = String::new();
        let mut ends = Vec::with_capacity(lines.len());
        let mut previous = None;
        for line in lines {
            if previous
                .is_some_and(|(number, end)| line.anchor.line != number || line.anchor.byte > end)
            {
                text.push('\n');
            }
            let start = text.len();
            for span in &line.spans {
                text.push_str(&span.text);
            }
            previous = Some((line.anchor.line, line.anchor.byte + text.len() - start));
            ends.push(text.len());
        }
        let row = pattern
            .find(&text)
            .map_or(0, |found| ends.partition_point(|end| *end <= found.start()));
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
            if event.button != Some(MouseButton::Left) {
                return;
            }
            if let Some(token) = approval_hit(&press, event.position) {
                press.approval_answer(token);
                event.stop_propagation();
                return;
            }
            if let Some(caret) = composer_offset(&press, event.position) {
                press.place(caret, event.modifiers.contains(ModifiersState::SHIFT));
                event.capture_pointer();
                event.stop_propagation();
            }
        })
        .pointer_move(move |event| {
            if event.is_captured()
                && let Some(caret) = composer_offset(&drag, event.position)
            {
                drag.place(caret, true);
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
    let content_left = margin(width);
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
    let prompt = usize::from(prompt_row(cols, rows, draft));
    let busy = busy_status(model);
    let body_height = usize::from(body_rows(cols, rows, draft, busy.is_some()));
    let mut history = app.history.borrow_mut();
    let visible = history.lines(model, content_width, model.entries.stamp);
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
            2,
            &[
                Span::text(" ".repeat(pad)),
                Span::styled(hint, palette().dim),
            ],
        );
    }
    for (offset, line) in visible.iter().skip(start).take(body_height).enumerate() {
        let start = (2 + offset) * width + content_left;
        for cell in &mut cells[start..start + content_width] {
            cell.bg = line.bg;
        }
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            2 + offset,
            &line.spans,
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
    if let Some((detail, persist)) = app.approval.peek() {
        let row = approval_row(&detail, content_width, persist);
        write_line_at(
            &mut cells,
            width,
            content_left,
            content_width,
            prompt.saturating_sub(1),
            &[Span::styled(row.text, palette().yellow)],
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
        &Input::new(draft, input::width(cols)),
        (app.caret.peek(), app.placement.get()),
        app.ticks.peek(),
        editing.as_deref(),
        app.select.peek(),
        app.editor.selection(),
        app.pane_active().is_none(),
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
    let content_left = margin(width);
    let body_height = usize::from(body_rows(
        cols,
        rows,
        &app.draft.peek(),
        model.busy_since.is_some(),
    ));
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
                            .inset_top((2 + first - start) as f32 * cell.h)
                            .width(placed.cols as f32 * cell.w)
                            .height(shown as f32 * cell.h),
                    )
                    .child(picture),
            )
        })
        .collect()
}

pub(super) struct ApprovalRow {
    pub text: String,
    pub allow: (usize, usize),
    pub always: Option<(usize, usize)>,
    pub deny: (usize, usize),
}

pub(super) fn approval_row(detail: &str, width: usize, persist: bool) -> ApprovalRow {
    const ALLOW: &str = "[ allow ]";
    const ALWAYS: &str = "[ always ]";
    const DENY: &str = "[ deny ]";
    let extra = if persist { cell_width(ALWAYS) + 1 } else { 0 };
    let room = width.saturating_sub(cell_width(ALLOW) + cell_width(DENY) + extra + 18);
    let mut text = format!("approve {}   ", ellipsize(detail, room));
    let allow_from = cell_width(&text);
    text.push_str(ALLOW);
    let allow_to = cell_width(&text);
    let always = persist.then(|| {
        text.push(' ');
        let from = cell_width(&text);
        text.push_str(ALWAYS);
        (from, cell_width(&text))
    });
    text.push(' ');
    let deny_from = cell_width(&text);
    text.push_str(DENY);
    let deny_to = cell_width(&text);
    text.push_str(if persist { "   y / Y / n" } else { "   y / n" });
    ApprovalRow {
        text,
        allow: (allow_from, allow_to),
        always,
        deny: (deny_from, deny_to),
    }
}

fn approval_hit(app: &App, point: Point) -> Option<&'static str> {
    let (detail, persist) = app.approval.peek()?;
    let cols = app.chat_cols();
    let width = usize::from(cols);
    let rows = app.viewport.peek().1;
    if width == 0 || rows < 5 {
        return None;
    }
    let content_left = margin(width);
    let content_width = width.saturating_sub(content_left * 2);
    let prompt = usize::from(prompt_row(cols, rows, &app.draft.peek())).saturating_sub(1);
    let row = approval_row(&detail, content_width, persist);
    let cell = canvas::cell();
    let col = (point.x / cell.w).max(0.0) as usize;
    let line = (point.y / cell.h).max(0.0) as usize;
    if line != prompt {
        return None;
    }
    if (content_left + row.allow.0..content_left + row.allow.1).contains(&col) {
        return Some("y");
    }
    if let Some((from, to)) = row.always
        && (content_left + from..content_left + to).contains(&col)
    {
        return Some("always");
    }
    if (content_left + row.deny.0..content_left + row.deny.1).contains(&col) {
        return Some("n");
    }
    None
}

fn empty_hint(width: usize) -> &'static str {
    [
        "ask anything · /login setup · @file attaches · !cmd runs · /help",
        "ask anything · /login · @file · /help",
        "ask anything · /login",
        "ask anything",
    ]
    .into_iter()
    .find(|hint| cell_width(hint) <= width)
    .unwrap_or("")
}

pub(super) fn input_rows(draft: &str, cols: u16, rows: u16) -> u16 {
    let limit = rows.saturating_sub(6).clamp(1, INPUT_ROWS);
    Input::new(draft, input::width(cols))
        .rows
        .len()
        .min(usize::from(limit)) as u16
}

pub(super) fn margin(width: usize) -> usize {
    if width > 100 {
        3
    } else if width > 28 {
        2
    } else {
        usize::from(width > 2)
    }
}

pub(super) fn prompt_row(cols: u16, rows: u16, draft: &str) -> u16 {
    rows.saturating_sub(input_rows(draft, cols, rows) + 3)
}

pub(super) fn body_rows(cols: u16, rows: u16, draft: &str, busy: bool) -> u16 {
    prompt_row(cols, rows, draft).saturating_sub(2 + u16::from(busy))
}

fn composer_offset(app: &App, point: Point) -> Option<Caret> {
    if app.view_edit.peek() {
        return None;
    }
    let draft = app.draft.peek();
    let cols = usize::from(app.chat_cols());
    let rows = app.viewport.peek().1;
    let left = margin(cols);
    let col = (point.x / canvas::cell().w).max(0.0) as usize;
    let row = (point.y / canvas::cell().h).max(0.0) as usize;
    let top = usize::from(prompt_row(cols as u16, rows, &draft)) + 1;
    let count = usize::from(input_rows(&draft, cols as u16, rows));
    if !(top..top + count).contains(&row) || !(left..cols.saturating_sub(left)).contains(&col) {
        return None;
    }
    let input = Input::new(&draft, input::width(cols as u16));
    let caret = input.caret(app.caret.peek(), app.placement.peek());
    let start = input.start(caret, count);
    let index = start + row - top;
    let range = input.rows.get(index)?;
    let line = &draft[range.clone()];
    let window = Draft::new(
        line,
        (index == caret.row).then_some(caret.byte.saturating_sub(range.start)),
        cols.saturating_sub(left * 2 + 6),
    );
    let byte = window.offset(col.saturating_sub(left + 4));
    input.hit(index, cell_width(&line[..byte]))
}

fn stamp_menu(cells: &mut [Cell], stride: usize, menu: &super::suggest::Menu) {
    for (offset, line) in menu.lines.iter().enumerate() {
        let y = menu.top.saturating_add(offset);
        let selected = line.chars().nth(1) == Some('›');
        let fg = if selected {
            palette().text
        } else {
            palette().gray
        };
        let bg = if selected {
            palette().user_bg
        } else {
            palette().code_bg
        };
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

fn header(model: &Model, width: usize, accent: Rgba8) -> Vec<Span> {
    let mut left = vec![
        Span::styled("summonr ", accent).bold(),
        Span::text(model.model.clone()),
    ];
    if width >= 80
        && let Some(relay) = model.relay.as_deref().filter(|relay| !relay.is_empty())
    {
        left.push(Span::styled(format!(" · {relay}"), palette().dim));
    }
    let mode = if model.snapshot.plan { "plan" } else { "auto" };
    let mode_color = if model.snapshot.plan {
        palette().cyan
    } else {
        palette().green
    };
    let mut right = Vec::new();
    if width >= 70 {
        right.push(Span::styled(
            format!("ses {} ", short_session(&model.session)),
            palette().dim,
        ));
    }
    right.push(Span::styled(format!("{mode} "), mode_color));
    if width >= 70 && model.snapshot.strict {
        right.push(Span::styled("strict ", palette().yellow));
    }
    if width >= 48 {
        right.push(Span::styled(
            format!(
                "{} tok ",
                fmt_tokens(model.prompt_tokens + model.completion_tokens)
            ),
            palette().dim,
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
    input: &Input<'_>,
    caret_position: (usize, Option<Caret>),
    ticks: u64,
    editing: Option<&str>,
    selecting: bool,
    selection: Option<Range<usize>>,
    focused: bool,
) {
    let [stride, origin, width, top] = area;
    let height = cells.len().checked_div(stride).unwrap_or(0);
    let count = height.saturating_sub(top + 3).max(1);
    let position = input.caret(caret_position.0, caret_position.1);
    let current = position.row;
    let start = input.start(position, count);
    let total = input.rows.len();
    let mode = if model.snapshot.plan { "plan" } else { "auto" };
    let mode_glyph = if model.snapshot.plan { '◇' } else { '●' };
    let mode_color = if model.snapshot.plan {
        palette().cyan
    } else {
        palette().green
    };
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
        palette().gray
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
            Span::styled(shown_model, palette().dim),
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
    for (index, range) in input.rows.iter().enumerate() {
        let line = &input.text[range.clone()];
        let line_start = range.start;
        let line_position = position.byte.saturating_sub(range.start).min(line.len());
        if index < start {
            continue;
        }
        if index >= start + count {
            break;
        }
        let window = Draft::new(
            line,
            (index == current && focused).then_some(line_position),
            interior.saturating_sub(2),
        );
        let (prompt, input) = match editing {
            Some(path) if index == start => (
                Span::text("  "),
                format!("editing {path} · ctrl+s save · esc leave"),
            ),
            Some(_) => (Span::text("  "), String::new()),
            None => (
                Span::styled(
                    if index == current && focused {
                        "❯ "
                    } else {
                        "  "
                    },
                    mode_color,
                ),
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
                    cell.bg = palette().user_bg;
                }
            }
        }
    }
    let options = model.prefs.hotkeys.label(crate::prefs::Hotkey::Options);
    let select = selecting.then(|| model.prefs.hotkeys.label(crate::prefs::Hotkey::Select));
    let hint = if !focused && !selecting {
        ellipsize(" pane controls · esc back ", width.saturating_sub(2))
    } else {
        composer_hint(
            model.busy_since.map(|(activity, _)| activity),
            width,
            &options,
            select.as_deref(),
        )
    };
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
            Span::styled(hint, palette().dim),
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
        Span::styled(rest.to_string(), palette().dim),
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
        if cell_width(text) <= available {
            let (before, after) = text.split_at(position);
            return Self {
                before,
                after,
                start: 0,
                position,
                left: false,
                right: false,
                cursor,
            };
        }
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

fn status(model: &Model, app: &App, width: usize, accent: Rgba8) -> Vec<Span> {
    let mut left = vec![
        Span::styled("📂 ", palette().dim),
        Span::styled(&app.info.workspace, palette().dim),
    ];
    if let Some(branch) = &app.git_branch {
        left.push(Span::styled("  ⎇ ", palette().dim));
        left.push(Span::styled(branch, palette().yellow));
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
    let mut right = usage(model, width);
    if line_width(&left) + line_width(&right) >= width {
        let name = Path::new(&app.info.workspace)
            .file_name()
            .unwrap_or_else(|| app.info.workspace.as_ref());
        left[1].text = name.to_string_lossy().into_owned();
    }
    if line_width(&left) + line_width(&right) >= width {
        left.truncate(2);
    }
    if line_width(&left) + line_width(&right) >= width {
        left.remove(0);
    }
    if !right.is_empty()
        && model.busy_since.is_none()
        && let Some(rate) = model.speed.last()
    {
        let speed = Span::styled(format!(" · {}", Speed::label(rate)), palette().dim);
        if line_width(&left) + line_width(&right) + speed.width() < width {
            right.push(speed);
        }
    }
    let spark = sparkline(&model.usage);
    if !right.is_empty()
        && !spark.is_empty()
        && line_width(&left) + line_width(&right) + cell_width(&spark) + 1 < width
    {
        right.insert(1, Span::styled(format!("{spark} "), accent));
    }
    let room = width.saturating_sub(line_width(&right) + 1);
    if line_width(&left) > room {
        if room < 4 {
            left.clear();
        } else {
            left[0].text = ellipsize(&left[0].text, room);
        }
    }
    join(left, right, width)
}

fn usage(model: &Model, width: usize) -> Vec<Span> {
    let tokens = format!(
        "{} tok",
        fmt_tokens(model.prompt_tokens + model.completion_tokens)
    );
    let mut full = vec![
        Span::styled(
            format!(
                "ctx {} · ",
                context(model.usage.last(), model.snapshot.context_window, false)
            ),
            palette().dim,
        ),
        Span::styled(format!("⚡ {tokens}"), palette().dim),
    ];
    if model.unreported_tokens > 0 {
        full.push(Span::styled(
            format!(" · {} unconfirmed", fmt_tokens(model.unreported_tokens)),
            palette().yellow,
        ));
    }
    if model.cached_tokens > 0 && model.prompt_tokens > 0 {
        full.push(Span::styled(
            format!(
                " · {}% cached",
                model.cached_tokens * 100 / model.prompt_tokens
            ),
            palette().dim,
        ));
    }
    if line_width(&full) <= width {
        return full;
    }
    let context = Span::styled(
        format!(
            "ctx {}",
            context(model.usage.last(), model.snapshot.context_window, true)
        ),
        palette().dim,
    );
    if context.width() > width {
        return Vec::new();
    }
    let mut compact = vec![context];
    for part in full
        .into_iter()
        .skip(2)
        .chain(once(Span::styled(format!(" · {tokens}"), palette().dim)))
    {
        if line_width(&compact) + part.width() <= width {
            compact.push(part);
        }
    }
    compact
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

fn join(mut left: Vec<Span>, right: Vec<Span>, width: usize) -> Vec<Span> {
    let right = clip_line(right, width);
    let right_width = line_width(&right);
    let gap = usize::from(right_width < width);
    left = clip_line(left, width.saturating_sub(right_width + gap));
    let used = line_width(&left) + right_width;
    left.push(Span::text(" ".repeat(width.saturating_sub(used))));
    left.extend(right);
    left
}

fn clip_line(mut line: Vec<Span>, width: usize) -> Vec<Span> {
    let mut used = 0;
    for span in &mut line {
        truncate(&mut span.text, width.saturating_sub(used));
        used += span.width();
    }
    line.retain(|span| !span.text.is_empty());
    line
}

fn line_width(line: &[Span]) -> usize {
    line.iter().map(Span::width).sum()
}

fn wrap(lines: Vec<Line>, width: usize, entry: usize, first_line: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        let anchor = Anchor {
            entry,
            line: first_line + index,
            byte: 0,
        };
        if line_width(&line.spans) <= width
            && line.spans.iter().all(|span| !span.text.contains('\n'))
        {
            rows.push(Row {
                spans: line.spans,
                bg: line.bg,
                anchor,
            });
            continue;
        }
        let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
        let mut spans = line.spans.into_iter().peekable();
        let mut offset = 0;
        for range in Input::new(&text, width).rows {
            let mut row = Vec::new();
            while let Some(span) = spans.peek() {
                let length = span.text.len();
                if offset + length <= range.start {
                    offset += length;
                    drop(spans.next());
                    continue;
                }
                if offset >= range.end {
                    break;
                }
                let start = range.start.saturating_sub(offset);
                let end = range.end.saturating_sub(offset).min(length);
                if start < end {
                    row.push(span.slice(start..end));
                }
                if end < length {
                    break;
                }
                offset += length;
                drop(spans.next());
            }
            rows.push(Row {
                spans: row,
                bg: line.bg,
                anchor: Anchor {
                    byte: range.start,
                    ..anchor
                },
            });
        }
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
        .style(Style::new().bg(palette().base));
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

pub(super) fn accent(_name: &str) -> Rgba8 {
    palette().accent
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
