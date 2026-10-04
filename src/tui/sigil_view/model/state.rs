use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(in crate::tui::sigil_view) struct Agent {
    pub(in crate::tui::sigil_view) id: String,
    pub(in crate::tui::sigil_view) call: String,
    pub(in crate::tui::sigil_view) background: bool,
    pub(in crate::tui::sigil_view) title: String,
    pub(in crate::tui::sigil_view) assignment: String,
    pub(in crate::tui::sigil_view) access: crate::child::Access,
    pub(in crate::tui::sigil_view) started: Instant,
    pub(in crate::tui::sigil_view) secs: Option<u64>,
    pub(in crate::tui::sigil_view) last: String,
    pub(in crate::tui::sigil_view) done: bool,
    pub(in crate::tui::sigil_view) failed: bool,
    pub(in crate::tui::sigil_view) tokens: u64,
    pub(in crate::tui::sigil_view) cost: f64,
}

impl Agent {
    pub(in crate::tui::sigil_view) fn placeholder(id: String) -> Self {
        Self {
            id,
            call: String::new(),
            background: false,
            title: String::new(),
            assignment: String::new(),
            access: crate::child::Access::default(),
            started: Instant::now(),
            secs: None,
            last: String::new(),
            done: false,
            failed: false,
            tokens: 0,
            cost: 0.0,
        }
    }

    pub(in crate::tui::sigil_view) fn elapsed(&self) -> u64 {
        self.secs
            .unwrap_or_else(|| self.started.elapsed().as_secs())
    }

    pub(in crate::tui::sigil_view) fn clock(&self) -> String {
        crate::text::duration(self.elapsed())
    }

    pub(in crate::tui::sigil_view) fn state(&self) -> ToolState {
        match (self.done, self.failed) {
            (_, true) => ToolState::Failed,
            (true, false) => ToolState::Ok,
            (false, false) => ToolState::Running,
        }
    }
}

#[derive(Clone, Default)]
pub(in crate::tui::sigil_view) struct Entries {
    pub(super) inner: Vec<Entry>,
    pub(in crate::tui::sigil_view) stamp: u64,
}

impl Entries {
    pub(in crate::tui::sigil_view) fn invalidate(&mut self) {
        self.stamp = self.stamp.wrapping_add(1);
    }

    pub(in crate::tui::sigil_view) fn push(&mut self, entry: Entry) {
        self.inner.push(entry);
    }

    pub(in crate::tui::sigil_view) fn clear(&mut self) {
        self.invalidate();
        self.inner.clear();
    }
}

#[derive(Clone)]
pub(in crate::tui::sigil_view) struct View {
    pub(in crate::tui::sigil_view) path: String,
    pub(in crate::tui::sigil_view) file: std::path::PathBuf,
    pub(in crate::tui::sigil_view) text: String,
    pub(in crate::tui::sigil_view) added: Vec<(usize, usize)>,
    pub(in crate::tui::sigil_view) image: Option<ImageSource>,
}
