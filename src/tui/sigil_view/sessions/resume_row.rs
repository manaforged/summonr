use sigil::prelude::{NodeBuilder, NodeEventsExt};

use super::super::ui::pick;
use super::{App, Picker, SessionInfo};

pub(super) fn row(
    app: App,
    picker: Picker,
    index: usize,
    info: &SessionInfo,
    selected: usize,
) -> NodeBuilder {
    let choose = app.clone();
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
    pick(index == selected, title, detail).pointer_down(move |event| {
        choose.sessions.selected.set(index);
        choose.resume_session();
        event.stop_propagation();
    })
}
