use sigil::prelude::NodeBuilder;

use super::super::ui::pick;
use super::{App, Picker, SessionInfo};

pub(super) fn row(app: &App, picker: Picker, info: &SessionInfo, selected: bool) -> NodeBuilder {
    let title = picker.title(info);
    let detail = app
        .model
        .peek()
        .live
        .iter()
        .find(|entry| entry.session == info.id)
        .map_or_else(
            || picker.detail(info),
            |entry| {
                let state = entry.status.label();
                let state = if state.is_empty() { "open" } else { state };
                format!("{state} · {}", picker.detail(info))
            },
        );
    pick(selected, title, detail)
}
