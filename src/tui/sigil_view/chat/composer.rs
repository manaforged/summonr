use std::time::Instant;

use sigil::app::cell_width;

use super::Speed;

pub(super) use super::{App, Model, suggest};
use crate::tui::Activity;

mod widget;

pub(super) use widget::widget;

pub(in crate::tui::sigil_view) fn busy_status(model: &Model, tick: u64) -> Option<String> {
    model.busy_since.map(|(activity, since)| {
        let doing = match &model.live_tool {
            Some((name, arg)) => format!("{name} {arg}").trim_end().to_string(),
            None => activity.label().to_string(),
        };
        let waiting = match model.queued.1 {
            0 => String::new(),
            n => format!(" · {n} queued · ↑ edits"),
        };
        let now = Instant::now();
        let speed = model
            .speed
            .live(now)
            .map(|rate| format!(" · {}", Speed::label(rate)))
            .unwrap_or_default();
        format!(
            "{} {doing} · {}{speed}{waiting}",
            sigil::widgets::spinner_frame(tick),
            crate::text::duration(since.elapsed().as_secs())
        )
    })
}

pub(in crate::tui::sigil_view) fn composer_hint(
    activity: Option<Activity>,
    width: usize,
    select: Option<&str>,
) -> String {
    let available = width.saturating_sub(2);
    let choices = if let Some(key) = select {
        vec![format!(" select on · mouse to terminal · {key} off ")]
    } else if activity == Some(Activity::Branch) {
        vec![" branching · draft kept ".to_string()]
    } else {
        vec![String::new()]
    };
    choices
        .into_iter()
        .find(|hint| cell_width(hint.as_str()) <= available)
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "composer_tests.rs"]
mod tests;
