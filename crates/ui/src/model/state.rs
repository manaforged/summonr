use std::ops::{Deref, DerefMut};

use super::*;

const TITLE_CHARS: usize = 56;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Doing {
    Thinking,
    Tool { name: String, arg: String },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Agent {
    pub(crate) id: String,
    pub(crate) call: String,
    pub(crate) background: bool,
    pub(crate) title: String,
    pub(crate) name: String,
    pub(crate) model: String,
    pub(crate) agent: String,
    pub(crate) assignment: String,
    pub(crate) workspace: String,
    pub(crate) access: summonr_agent::child::Access,
    pub(crate) started: Instant,
    pub(crate) secs: Option<u64>,
    pub(crate) last: String,
    pub(crate) done: bool,
    pub(crate) failed: bool,
    pub(crate) tokens: u64,
    pub(crate) cost: f64,
}

impl Agent {
    pub(crate) fn placeholder(id: String) -> Self {
        Self {
            id,
            call: String::new(),
            background: false,
            title: String::new(),
            name: String::new(),
            model: String::new(),
            agent: String::new(),
            assignment: String::new(),
            workspace: String::new(),
            access: summonr_agent::child::Access::default(),
            started: Instant::now(),
            secs: None,
            last: String::new(),
            done: false,
            failed: false,
            tokens: 0,
            cost: 0.0,
        }
    }

    pub(crate) fn title_of(assignment: &str) -> String {
        let line = assignment.lines().next().unwrap_or_default().trim();
        let line = line
            .split_once(": ")
            .filter(|(label, _)| label.chars().all(|c| c.is_ascii_uppercase() || c == ' '))
            .map_or(line, |(_, goal)| goal);
        let clause = [". ", ": ", " (", ", ", " — "]
            .iter()
            .filter_map(|mark| line.find(mark))
            .min()
            .map_or(line, |at| &line[..at]);
        crate::cells::ellipsize(clause.trim_end_matches('.'), TITLE_CHARS).to_string()
    }

    pub(crate) fn roots(&self, roots: &[String]) -> Vec<String> {
        std::iter::once(&self.workspace)
            .filter(|workspace| !workspace.is_empty())
            .chain(roots)
            .cloned()
            .collect()
    }

    pub(crate) fn elapsed(&self) -> u64 {
        self.secs
            .unwrap_or_else(|| self.started.elapsed().as_secs())
    }

    pub(crate) fn clock(&self) -> String {
        summonr_core::text::duration(self.elapsed())
    }

    pub(crate) fn state(&self) -> ToolState {
        match (self.done, self.failed) {
            (_, true) => ToolState::Failed,
            (true, false) => ToolState::Ok,
            (false, false) => ToolState::Running,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Entries {
    pub(crate) inner: Vec<Entry>,
    revs: Vec<u64>,
    clock: u64,
    asked: bool,
    pub(crate) stamp: u64,
}

impl Entries {
    pub(crate) fn invalidate(&mut self) {
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

    pub(crate) fn rev(&self, index: usize) -> u64 {
        self.revs.get(index).copied().unwrap_or_default()
    }

    pub(crate) fn asked(&self) -> bool {
        self.asked
            || self
                .inner
                .get(self.revs.len()..)
                .unwrap_or_default()
                .iter()
                .any(|entry| matches!(entry, Entry::User(_)))
    }

    pub(crate) fn touch(&mut self, index: usize) {
        self.sync();
        let rev = self.tick();
        if let Some(slot) = self.revs.get_mut(index) {
            *slot = rev;
        }
    }

    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut Entry> {
        self.touch(index);
        self.inner.get_mut(index)
    }

    pub(crate) fn last_mut(&mut self) -> Option<&mut Entry> {
        self.get_mut(self.inner.len().checked_sub(1)?)
    }

    pub(crate) fn push(&mut self, entry: Entry) {
        self.inner.push(entry);
        self.sync();
    }

    pub(crate) fn insert(&mut self, index: usize, entry: Entry) {
        self.sync();
        self.inner.insert(index, entry);
        self.revs.truncate(index);
        self.sync();
    }

    pub(crate) fn clear(&mut self) {
        self.invalidate();
        self.inner.clear();
        self.revs.clear();
        self.asked = false;
    }
}

#[derive(Clone)]
pub(crate) struct View {
    pub(crate) path: String,
    pub(crate) file: std::path::PathBuf,
    pub(crate) text: String,
    pub(crate) added: Vec<(usize, usize)>,
    pub(crate) image: Option<ImageSource>,
}

impl Deref for Entries {
    type Target = [Entry];

    fn deref(&self) -> &[Entry] {
        &self.inner
    }
}

impl DerefMut for Entries {
    fn deref_mut(&mut self) -> &mut [Entry] {
        &mut self.inner
    }
}

impl<'a> IntoIterator for &'a Entries {
    type Item = &'a Entry;
    type IntoIter = std::slice::Iter<'a, Entry>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl Extend<Entry> for Entries {
    fn extend<I: IntoIterator<Item = Entry>>(&mut self, iter: I) {
        self.inner.extend(iter);
    }
}
