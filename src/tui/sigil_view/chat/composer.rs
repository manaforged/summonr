use std::ops::Range;
use std::time::Instant;

use sigil_tui::cell_width;
use unicode_segmentation::UnicodeSegmentation;

use super::super::app::App;
use super::super::canvas::{self, palette};
use super::super::input::{self, Caret, Input};
use super::super::model::Model;
use super::super::speed::Speed;
use super::{Cell, Span, accent, ellipsize, write_line_at};
use crate::tui::Activity;
use sigil::prelude::{Point, Px, Rgba8, StyleAtom, StyleDimension};

const INPUT_ROWS: u16 = 8;
pub(in crate::tui::sigil_view) const INPUT_HEIGHTS: [[StyleAtom; 1]; INPUT_ROWS as usize] = {
    let mut heights = [[StyleAtom::Height(StyleDimension::Auto)]; INPUT_ROWS as usize];
    let mut index = 0;
    while index < heights.len() {
        heights[index] = [StyleAtom::Height(StyleDimension::Px(Px(
            ((index + 4) as f32 * sigil_tui::Host::CELL) as i16,
        )))];
        index += 1;
    }
    heights
};
pub(in crate::tui::sigil_view) const SPINNER: [&str; 10] =
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub(super) struct Prompt<'a> {
    pub cells: &'a mut [Cell],
    pub stride: usize,
    pub origin: usize,
    pub width: usize,
    pub top: usize,
    pub model: &'a Model,
    pub input: &'a Input<'a>,
    pub caret: (usize, Option<Caret>),
    pub ticks: u64,
    pub editing: Option<&'a str>,
    pub selecting: bool,
    pub selection: Option<Range<usize>>,
    pub focused: bool,
}

pub(super) fn prompt_rows(prompt: Prompt<'_>) {
    let Prompt {
        cells,
        stride,
        origin,
        width,
        top,
        model,
        input,
        caret: caret_position,
        ticks,
        editing,
        selecting,
        selection,
        focused,
    } = prompt;
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
    let suffix = model
        .snapshot
        .effort
        .filter(|effort| *effort != crate::provider::Effort::None)
        .map_or(String::new(), |effort| format!("({})", effort.as_str()));
    let shown_model = ellipsize(
        &model.snapshot.model,
        model_width.saturating_sub(cell_width(&suffix)),
    );
    let fill = width.saturating_sub(
        cell_width(left.as_str()) + cell_width(&shown_model) + cell_width(&suffix) + 2,
    );
    write_line_at(
        cells,
        stride,
        origin,
        width,
        top + 1,
        &[
            Span::styled("╭", border),
            Span::styled(format!(" {mode_glyph} {mode}{range} "), mode_color),
            Span::styled("─".repeat(fill), border),
            Span::styled(shown_model, palette().dim),
            Span::styled(suffix, palette().yellow).bold(),
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
            top + 2 + index - start,
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
            let row = top + 2 + index - start;
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
        top + 2 + count,
        &[
            Span::styled("╰", border),
            Span::styled("─".repeat(dash), border),
            Span::styled(hint, palette().gray),
            Span::styled("╯", border),
        ],
    );
}

pub(in crate::tui::sigil_view) fn busy_status(model: &Model) -> Option<String> {
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
        let now = Instant::now();
        let speed = model
            .speed
            .live(now)
            .map(|rate| format!(" · {}", Speed::label(rate)))
            .unwrap_or_default();
        format!(
            "{} {doing} · {}s{speed}{waiting}",
            SPINNER[model.spinner],
            since.elapsed().as_secs()
        )
    })
}

pub(super) fn attachment_line(attached: usize) -> Vec<Span> {
    let noun = if attached == 1 { "image" } else { "images" };
    vec![
        Span::styled("📷 ", palette().gray),
        Span::styled(format!("{attached} {noun} attached"), palette().accent),
        Span::styled(" · enter sends with your prompt", palette().gray),
    ]
}

pub(super) fn busy_line(status: &str, width: usize, accent: Rgba8) -> Vec<Span> {
    let shown = ellipsize(status, width);
    let (spinner, rest) = shown.split_at(shown.find(' ').unwrap_or(shown.len()));
    vec![
        Span::styled(spinner.to_string(), accent),
        Span::styled(rest.to_string(), palette().text),
    ]
}

pub(in crate::tui::sigil_view) fn composer_hint(
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

pub(in crate::tui::sigil_view) fn draft_view(
    draft: &str,
    position: usize,
    width: usize,
    caret: char,
) -> String {
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

pub(super) fn empty_hint(width: usize) -> &'static str {
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

pub(in crate::tui::sigil_view) fn input_rows(draft: &str, cols: u16, rows: u16) -> u16 {
    let limit = rows.saturating_sub(6).clamp(1, INPUT_ROWS);
    Input::new(draft, input::width(cols))
        .rows
        .len()
        .min(usize::from(limit)) as u16
}

pub(in crate::tui::sigil_view) fn margin(width: usize) -> usize {
    if width > 100 {
        4
    } else if width > 56 {
        3
    } else if width > 28 {
        2
    } else {
        usize::from(width > 2)
    }
}

pub(in crate::tui::sigil_view) const FOOTER_GAP: u16 = 1;
pub(in crate::tui::sigil_view) const FOOTER_PAD: u16 = 1;
pub(in crate::tui::sigil_view) const FOOTER_ROWS: u16 = 1;
pub(in crate::tui::sigil_view) const STATUS_GAP: u16 = 1;

pub(in crate::tui::sigil_view) fn overlay_rows(rows: u16) -> u16 {
    rows.saturating_sub(FOOTER_GAP + FOOTER_PAD + FOOTER_ROWS)
}

pub(in crate::tui::sigil_view) fn prompt_row(cols: u16, rows: u16, draft: &str) -> u16 {
    rows.saturating_sub(input_rows(draft, cols, rows) + 3)
}

pub(in crate::tui::sigil_view) fn body_rows(cols: u16, rows: u16, draft: &str, busy: bool) -> u16 {
    prompt_row(cols, rows, draft).saturating_sub(2 + u16::from(busy) + STATUS_GAP)
}

pub(super) fn composer_offset(app: &App, point: Point) -> Option<Caret> {
    if app.view_edit.peek() {
        return None;
    }
    let draft = app.draft.peek();
    let cols = usize::from(app.chat_cols());
    let rows = overlay_rows(app.viewport.peek().1);
    let left = margin(cols);
    let col = (point.x / canvas::cell().w).max(0.0) as usize;
    let row = (point.y / canvas::cell().h).max(0.0) as usize;
    let top = usize::from(prompt_row(cols as u16, rows, &draft)) + 2;
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
