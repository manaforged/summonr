use super::*;
use crate::catalog::{Cost, Model};

const LONG_CONTEXT: u32 = 1_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Facet {
    #[default]
    All,
    Ready,
    Favorites,
    Plan,
    Free,
    Think,
    Vision,
    Long,
}

impl Facet {
    pub(in crate::tui::sigil_view) const ALL: [Self; 8] = [
        Self::All,
        Self::Ready,
        Self::Favorites,
        Self::Plan,
        Self::Free,
        Self::Think,
        Self::Vision,
        Self::Long,
    ];

    pub(in crate::tui::sigil_view) const fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Ready => "ready",
            Self::Favorites => "★",
            Self::Plan => "plan",
            Self::Free => "free",
            Self::Think => "think",
            Self::Vision => "vision",
            Self::Long => "≥1M",
        }
    }

    fn keeps(self, fact: Option<&Model>, state: Option<State>, pinned: bool) -> bool {
        match self {
            Self::All => true,
            Self::Ready => state.is_none_or(|state| !state.blocked()),
            Self::Favorites => pinned,
            Self::Plan => fact.is_some_and(|fact| fact.cost == Cost::Plan),
            Self::Free => fact.is_some_and(|fact| fact.cost == Cost::Free),
            Self::Think => fact.is_some_and(|fact| fact.reasoning),
            Self::Vision => fact.is_some_and(|fact| fact.vision),
            Self::Long => fact
                .and_then(|fact| fact.context)
                .is_some_and(|tokens| tokens >= LONG_CONTEXT),
        }
    }
}

impl App {
    pub(in crate::tui::sigil_view) fn cycle_facet(&self, delta: isize) {
        let at = Facet::ALL
            .iter()
            .position(|facet| *facet == self.picker.facet.peek())
            .unwrap_or(0);
        self.pick_facet(Facet::ALL[crate::tui::sigil_view::ui::cycle(at, delta, Facet::ALL.len())]);
    }

    pub(in crate::tui::sigil_view) fn pick_facet(&self, facet: Facet) {
        self.picker.facet.set(facet);
        self.rank();
        self.picker.note.set(String::new());
        self.picker.list.cursor.set(0);
    }

    pub(in crate::tui::sigil_view) fn facet_counts(&self) -> Vec<(Facet, usize)> {
        Facet::ALL
            .into_iter()
            .map(|facet| (facet, self.ranked(facet).0.len()))
            .collect()
    }

    pub(super) fn faceted(&self, rows: Vec<ModelId>, facet: Facet) -> Vec<ModelId> {
        if facet == Facet::All {
            return rows;
        }
        let facts = self.picker.facts.peek();
        let groups = self.picker.groups.peek();
        let mut seen = std::collections::BTreeSet::new();
        rows.into_iter()
            .filter(|selection| {
                facet.keeps(
                    facts.get(selection),
                    groups.get(&selection.endpoint).copied(),
                    self.favorite(selection),
                ) && seen.insert(selection.clone())
            })
            .collect()
    }
}
