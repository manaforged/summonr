pub(super) use super::{App, Model, suggest};
use sigil::app::cell_width;
use summonr_agent::hub::Activity;

mod widget;

pub(super) use widget::widget;

pub(crate) fn composer_hint(
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
