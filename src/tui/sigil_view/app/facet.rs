use std::collections::{BTreeMap, BTreeSet};

use sigil::prelude::fuzzy_score;

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
        self.picker.note.set(String::new());
        self.picker.list.cursor.set(0);
    }

    pub(in crate::tui::sigil_view) fn facet_counts(&self) -> Vec<(Facet, usize)> {
        let choices = self.picker.choices.get();
        let counts = self.picker.recent.get();
        let facts = self.picker.facts.get();
        let groups = self.picker.groups.get();
        let query = self.picker.list.query.get();
        let older = self.picker.older.get();
        Facet::ALL
            .into_iter()
            .map(|facet| {
                (
                    facet,
                    ranked(&choices, counts, &facts, &groups, &query, facet, older)
                        .0
                        .len(),
                )
            })
            .collect()
    }
}

pub(super) fn ranked(
    choices: &[ModelId],
    counts: [usize; 3],
    facts: &BTreeMap<ModelId, Model>,
    groups: &BTreeMap<String, State>,
    query: &str,
    facet: Facet,
    older: bool,
) -> (Vec<ModelId>, usize) {
    let query = query.to_lowercase();
    if query.trim().is_empty() {
        let (rows, hidden) =
            picker_rows(choices, counts, facts, groups, older, facet == Facet::All);
        return (faceted(rows, facet, choices, counts, facts, groups), hidden);
    }
    let (rows, _) = picker_rows(choices, counts, facts, groups, true, false);
    let rows = faceted(rows, facet, choices, counts, facts, groups);
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut ranked: Vec<(usize, ModelId)> = rows
        .into_iter()
        .filter_map(|row| {
            let display = facts.get(&row).map_or("", |fact| fact.label());
            let rank = words
                .iter()
                .map(|word| score(word, &row, display))
                .try_fold(0, |worst, rank| Some(worst.max(rank?)))?;
            Some((rank, row))
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    (ranked.into_iter().map(|(_, row)| row).collect(), 0)
}

fn faceted(
    rows: Vec<ModelId>,
    facet: Facet,
    choices: &[ModelId],
    [current, pins, _]: [usize; 3],
    facts: &BTreeMap<ModelId, Model>,
    groups: &BTreeMap<String, State>,
) -> Vec<ModelId> {
    if facet == Facet::All {
        return rows;
    }
    let pinned: BTreeSet<&ModelId> = choices.iter().skip(current).take(pins).collect();
    let mut seen = BTreeSet::new();
    rows.into_iter()
        .filter(|selection| {
            facet.keeps(
                facts.get(selection),
                groups.get(&selection.endpoint).copied(),
                pinned.contains(selection),
            ) && seen.insert(selection.clone())
        })
        .collect()
}

fn picker_rows(
    choices: &[ModelId],
    counts: [usize; 3],
    facts: &BTreeMap<ModelId, Model>,
    groups: &BTreeMap<String, State>,
    older: bool,
    collapse: bool,
) -> (Vec<ModelId>, usize) {
    let listed: usize = counts.iter().sum();
    let (held, rest) = choices.split_at(listed.min(choices.len()));
    let offline = |selection: &ModelId| groups.get(&selection.endpoint) == Some(&State::Offline);
    let served: Vec<(&ModelId, Option<&Model>)> = rest
        .iter()
        .map(|selection| (selection, facts.get(selection)))
        .filter(|(selection, fact)| !offline(selection) && fact.is_none_or(|fact| fact.usable()))
        .collect();
    let models: Vec<(&str, &Model)> = served
        .iter()
        .filter_map(|(selection, fact)| Some((selection.endpoint.as_str(), (*fact)?)))
        .collect();
    let mut current = crate::catalog::current(&models).into_iter();
    let mut fixes = BTreeSet::new();
    let mut hidden = 0;
    let mut kept = Vec::new();
    for (selection, fact) in &served {
        let now = fact.is_none() || current.next().unwrap_or(true);
        if collapse
            && groups
                .get(&selection.endpoint)
                .is_some_and(|state| state.blocked())
        {
            if fixes.insert(selection.endpoint.as_str()) {
                kept.push((*selection).clone());
            }
            continue;
        }
        hidden += usize::from(!now);
        if older || now {
            kept.push((*selection).clone());
        }
    }
    (held.iter().cloned().chain(kept).collect(), hidden)
}

fn score(query: &str, row: &ModelId, display: &str) -> Option<usize> {
    let query = match query.split_once('/') {
        Some((want, rest)) if row.endpoint.to_lowercase().starts_with(want) => rest,
        Some(_) | None => query,
    };
    let hay = format!("{}/{} {display}", row.endpoint, row.model);
    let id = fuzzy_score(query, &row.model).map(|found| found.score);
    let hay = fuzzy_score(query, &hay).map(|found| found.score);
    id.filter(|score| *score <= 10)
        .or(hay.filter(|score| *score < 40).map(|score| score.max(20)))
        .or(id)
        .or(hay.map(|score| score + 60))
}
