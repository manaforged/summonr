#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::tui::sigil_view) struct Note {
    pub(in crate::tui::sigil_view) text: String,
    pub(in crate::tui::sigil_view) failed: bool,
}

impl Note {
    pub(in crate::tui::sigil_view) const fn plain(text: String) -> Self {
        Self {
            text,
            failed: false,
        }
    }

    pub(super) const fn failed(text: String) -> Self {
        Self { text, failed: true }
    }
}
