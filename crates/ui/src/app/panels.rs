use std::collections::BTreeMap;

use sigil::prelude::{Signal, signal};
use summonr_agent::hub::{Fact, Msg};
use summonr_agent::panel::{Content, Panel};

#[derive(Clone, Copy)]
pub(crate) struct Panels {
    pub(crate) list: Signal<Vec<Panel>>,
    pub(crate) shown: Signal<Option<String>>,
    pub(crate) content: Signal<Option<Result<Content, String>>>,
    /// The last output of every panel that ran, by name.
    pub(crate) results: Signal<BTreeMap<String, Result<Content, String>>>,
}

impl Panels {
    pub(super) fn new() -> Self {
        Self {
            list: signal(Vec::new()),
            shown: signal(None),
            content: signal(None),
            results: signal(BTreeMap::new()),
        }
    }

    pub(super) fn take(&self, msg: Msg) -> Option<Msg> {
        match msg {
            Msg::Fact(Fact::Panels(list)) => self.listed(list),
            Msg::Fact(Fact::Panel { name, result }) => self.loaded(&name, result),
            msg => return Some(msg),
        }
        None
    }

    fn listed(&self, list: Vec<Panel>) {
        if self.list.with(|current| *current != list) {
            self.list.set(list);
        }
    }

    fn loaded(&self, name: &str, result: Result<Content, String>) {
        if self.shown.with(|shown| shown.as_deref() == Some(name)) {
            self.content.set(Some(result.clone()));
        }
        if self.results.with(|all| all.get(name) != Some(&result)) {
            self.results.update(|all| drop(all.insert(name.to_owned(), result)));
        }
    }
}
