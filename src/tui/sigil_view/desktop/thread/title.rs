use super::*;

pub(super) fn active(app: &App) -> Option<crate::tui::hub::Live> {
    app.model
        .get()
        .live
        .iter()
        .find(|entry| entry.active)
        .cloned()
}
