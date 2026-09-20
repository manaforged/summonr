use std::time::Instant;

use sigil_tui::cell_width;

use super::Speed;
use super::canvas::ellipsize;

pub(super) use super::{App, Model, suggest};
use crate::tui::Activity;

use draft::Draft;

mod draft;
mod lines;
mod widget;

pub(super) use widget::widget;

pub(in crate::tui::sigil_view) const SPINNER: [&str; 10] =
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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
            SPINNER[model.spinner % SPINNER.len()],
            since.elapsed().as_secs()
        )
    })
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
