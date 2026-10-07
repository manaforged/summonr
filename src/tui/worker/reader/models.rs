use std::collections::BTreeMap;

use futures::StreamExt;
use futures::stream::FuturesUnordered;

use super::{Msg, Reader, add};
use crate::catalog::Model;
use crate::endpoint::{self, State};
use crate::prompt::summonr_home;
use crate::provider::Client;
use crate::recent::{self, Seat};

fn waiting(ready: &[endpoint::Endpoint], errors: &mut Vec<String>) -> Vec<(String, String, State)> {
    let Ok(names) = endpoint::names() else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|name| !ready.iter().any(|endpoint| &endpoint.name == name))
        .filter_map(|name| {
            let found = endpoint::auth_info(&name).and_then(|info| {
                Ok((
                    info,
                    endpoint::resolve(Some(&name), None, endpoint::Need::Config)?,
                ))
            });
            let (info, endpoint) = match found {
                Ok(found) => found,
                Err(error) => {
                    errors.push(format!("{name}: {error}"));
                    return None;
                }
            };
            let state = if info.flow.signs_in() {
                State::SignIn
            } else {
                State::NeedsKey
            };
            (!info.configured && !info.keyless() && !endpoint.model.is_empty()).then_some((
                name,
                endpoint.model,
                state,
            ))
        })
        .collect()
}

fn order(rows: &mut [Seat], facts: &BTreeMap<Seat, Model>, groups: &BTreeMap<String, State>) {
    let rank = |row: &Seat| facts.get(row).map_or(2, |model| model.cost.rank());
    let mut best: BTreeMap<String, u8> = BTreeMap::new();
    for row in rows.iter() {
        let rank = rank(row);
        best.entry(row.endpoint.clone())
            .and_modify(|held| *held = rank.min(*held))
            .or_insert(rank);
    }
    rows.sort_by_cached_key(|row| {
        let name = row.endpoint.clone();
        let state = groups.get(&name).map_or(0, |state| {
            u8::from(state.setup()) * 2 + u8::from(state.failed())
        });
        (
            state,
            best.get(&name).copied(),
            name,
            rank(row),
            std::cmp::Reverse(facts.get(row).and_then(|model| model.release)),
            row.clone(),
        )
    });
}

fn enrich(endpoints: &[endpoint::Endpoint], facts: &mut BTreeMap<Seat, Model>) {
    for endpoint in endpoints {
        let mut models: Vec<Model> = facts
            .iter()
            .filter(|(row, _)| row.endpoint == endpoint.name)
            .map(|(_, model)| model.clone())
            .collect();
        crate::catalog::enrich(&endpoint.name, endpoint.base_url.as_deref(), &mut models);
        for model in models {
            facts.insert(Seat::new(&endpoint.name, &model.id), model);
        }
    }
}

struct Board {
    rows: Vec<Seat>,
    top: usize,
    sorted: bool,
    facts: BTreeMap<Seat, Model>,
    groups: BTreeMap<String, State>,
    causes: BTreeMap<String, String>,
}

impl Board {
    fn seed(home: Option<&std::path::Path>, names: &[&str]) -> Self {
        let known = |row: &Seat| names.contains(&row.endpoint.as_str());
        let mut rows: Vec<Seat> = home.map(recent::pinned).unwrap_or_default();
        rows.retain(known);
        let recents = home.map(recent::load).unwrap_or_default();
        for row in recents.into_iter().filter(known).take(5) {
            add(&mut rows, row);
        }
        Self {
            top: rows.len(),
            sorted: false,
            rows,
            facts: BTreeMap::new(),
            groups: BTreeMap::new(),
            causes: BTreeMap::new(),
        }
    }

    fn cached(&mut self, endpoints: &[endpoint::Endpoint], checked: impl Fn(&str) -> bool) {
        let kept = crate::catalog::cached();
        for endpoint in endpoints {
            let listing = kept.get(&endpoint.name);
            if checked(&endpoint.name) {
                let state = if listing.is_some() {
                    State::Checking
                } else {
                    State::Loading
                };
                self.groups.insert(endpoint.name.clone(), state);
            }
            if !endpoint.model.is_empty() {
                add(&mut self.rows, Seat::new(&endpoint.name, &endpoint.model));
            }
            for model in listing.into_iter().flat_map(|listing| &listing.models) {
                let row = Seat::new(&endpoint.name, &model.id);
                self.facts.insert(row.clone(), model.clone());
                add(&mut self.rows, row);
            }
        }
    }

    fn waiting(&mut self, setup: &[(String, String, State)]) {
        for (name, model, state) in setup {
            self.groups.insert(name.clone(), *state);
            add(&mut self.rows, Seat::new(name, model));
        }
    }

    fn snapshot(
        &mut self,
        request: u64,
        home: Option<&std::path::Path>,
        loading: bool,
        errors: &[String],
    ) -> Msg {
        let top = self.top;
        if !self.sorted {
            order(&mut self.rows[top..], &self.facts, &self.groups);
            self.sorted = true;
        }
        let mut sent: Vec<Seat> = home.map(recent::pinned).unwrap_or_default();
        sent.retain(|row| self.rows.contains(row));
        let pinned = sent.len();
        for row in &self.rows[..top] {
            add(&mut sent, row.clone());
        }
        let head = sent.len();
        for row in &self.rows[top..] {
            add(&mut sent, row.clone());
        }
        Msg::Models {
            request,
            pinned,
            recent: head - pinned,
            rows: sent,
            facts: self.facts.clone(),
            groups: self.groups.clone(),
            causes: self.causes.clone(),
            loading,
            error: errors.join("; "),
        }
    }

    fn discovered(&mut self, name: String, models: Vec<Model>) {
        for model in models {
            let row = Seat::new(&name, &model.id);
            self.facts.insert(row.clone(), model);
            add(&mut self.rows, row);
        }
        self.causes.remove(&name);
        self.groups.insert(name, State::Ready);
    }
}

impl Reader {
    pub(super) async fn models(&self, request: u64, only: Option<String>) {
        let (endpoints, mut errors) = endpoint::ready(endpoint::Need::Key);
        let setup = waiting(&endpoints, &mut errors);
        let names: Vec<&str> = endpoints
            .iter()
            .map(|endpoint| endpoint.name.as_str())
            .chain(setup.iter().map(|(name, ..)| name.as_str()))
            .collect();
        let home = summonr_home().ok();
        let mut board = Board::seed(home.as_deref(), &names);
        let checked = |name: &str| only.as_deref().is_none_or(|only| only == name);
        board.cached(&endpoints, checked);
        board.waiting(&setup);
        let mut registry = Some(tokio::spawn(crate::catalog::refresh_registry()));
        let mut pending: FuturesUnordered<_> = endpoints
            .iter()
            .filter(|endpoint| checked(&endpoint.name))
            .map(
                |endpoint| async move { (endpoint.name.clone(), Client::discover(endpoint).await) },
            )
            .collect();
        loop {
            self.send(board.snapshot(request, home.as_deref(), !pending.is_empty(), &errors));
            let next = match registry.as_mut() {
                Some(fetch) if pending.is_empty() => Err(fetch.await),
                Some(fetch) => tokio::select! {
                    done = fetch => Err(done),
                    next = pending.next() => Ok(next),
                },
                None => Ok(pending.next().await),
            };
            let (name, result) = match next {
                Ok(Some(next)) => next,
                Ok(None) => return,
                Err(done) => {
                    registry = None;
                    if matches!(done, Ok(Ok(()))) {
                        enrich(&endpoints, &mut board.facts);
                    }
                    continue;
                }
            };
            match result {
                Ok(models) => board.discovered(name, models),
                Err(error) => {
                    board.groups.insert(name.clone(), Client::state(&error));
                    board.causes.insert(name, error.to_string());
                }
            }
        }
    }
}
