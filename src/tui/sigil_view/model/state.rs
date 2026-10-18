use super::*;

const TITLE_CHARS: usize = 56;

#[derive(Clone, Debug, PartialEq)]
pub(in crate::tui::sigil_view) struct Agent {
    pub(in crate::tui::sigil_view) id: String,
    pub(in crate::tui::sigil_view) call: String,
    pub(in crate::tui::sigil_view) background: bool,
    pub(in crate::tui::sigil_view) title: String,
    pub(in crate::tui::sigil_view) agent: String,
    pub(in crate::tui::sigil_view) assignment: String,
    pub(in crate::tui::sigil_view) workspace: String,
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
            agent: String::new(),
            assignment: String::new(),
            workspace: String::new(),
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

    pub(in crate::tui::sigil_view) fn title_of(assignment: &str) -> String {
        let line = assignment.lines().next().unwrap_or_default().trim();
        let clause = [". ", ": ", " (", ", ", " — "]
            .iter()
            .filter_map(|mark| line.find(mark))
            .min()
            .map_or(line, |at| &line[..at]);
        crate::text::ellipsize(clause.trim_end_matches('.'), TITLE_CHARS).to_string()
    }

    pub(in crate::tui::sigil_view) fn roots(&self, roots: &[String]) -> Vec<String> {
        std::iter::once(&self.workspace)
            .filter(|workspace| !workspace.is_empty())
            .chain(roots)
            .cloned()
            .collect()
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
    revs: Vec<u64>,
    clock: u64,
    asked: bool,
    pub(in crate::tui::sigil_view) stamp: u64,
}

impl Entries {
    pub(in crate::tui::sigil_view) fn invalidate(&mut self) {
        self.stamp = self.stamp.wrapping_add(1);
    }

    fn tick(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    fn sync(&mut self) {
        while self.revs.len() < self.inner.len() {
            self.asked |= matches!(self.inner[self.revs.len()], Entry::User(_));
            let rev = self.tick();
            self.revs.push(rev);
        }
        self.revs.truncate(self.inner.len());
    }

    fn reask(&mut self) {
        self.asked = self
            .inner
            .iter()
            .any(|entry| matches!(entry, Entry::User(_)));
    }

    pub(in crate::tui::sigil_view) fn rev(&self, index: usize) -> u64 {
        self.revs.get(index).copied().unwrap_or_default()
    }

    pub(in crate::tui::sigil_view) fn asked(&self) -> bool {
        self.asked
            || self
                .inner
                .get(self.revs.len()..)
                .unwrap_or_default()
                .iter()
                .any(|entry| matches!(entry, Entry::User(_)))
    }

    pub(in crate::tui::sigil_view) fn touch(&mut self, index: usize) {
        self.sync();
        let rev = self.tick();
        if let Some(slot) = self.revs.get_mut(index) {
            *slot = rev;
        }
    }

    pub(in crate::tui::sigil_view) fn get_mut(&mut self, index: usize) -> Option<&mut Entry> {
        self.touch(index);
        self.inner.get_mut(index)
    }

    pub(in crate::tui::sigil_view) fn last_mut(&mut self) -> Option<&mut Entry> {
        self.get_mut(self.inner.len().checked_sub(1)?)
    }

    pub(in crate::tui::sigil_view) fn push(&mut self, entry: Entry) {
        self.inner.push(entry);
        self.sync();
    }

    pub(in crate::tui::sigil_view) fn insert(&mut self, index: usize, entry: Entry) {
        self.sync();
        self.inner.insert(index, entry);
        self.revs.truncate(index);
        self.sync();
    }

    pub(in crate::tui::sigil_view) fn remove(&mut self, index: usize) -> Entry {
        self.sync();
        let entry = self.inner.remove(index);
        self.revs.truncate(index);
        self.sync();
        self.reask();
        entry
    }

    pub(in crate::tui::sigil_view) fn pop(&mut self) -> Option<Entry> {
        self.sync();
        let entry = self.inner.pop();
        self.sync();
        if matches!(entry, Some(Entry::User(_))) {
            self.reask();
        }
        entry
    }

    pub(in crate::tui::sigil_view) fn clear(&mut self) {
        self.invalidate();
        self.inner.clear();
        self.revs.clear();
        self.asked = false;
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
