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

impl Reader {
    pub(super) async fn models(&self, request: u64, only: Option<String>) {
        let (endpoints, mut errors) = endpoint::ready(endpoint::Need::Key);
        let setup = waiting(&endpoints, &mut errors);
        let names: Vec<&str> = endpoints
            .iter()
            .map(|endpoint| endpoint.name.as_str())
            .chain(setup.iter().map(|(name, ..)| name.as_str()))
            .collect();
        let known = |row: &Seat| names.contains(&row.endpoint.as_str());
        let home = summonr_home().ok();
        let mut rows: Vec<Seat> = home.as_deref().map(recent::pinned).unwrap_or_default();
        rows.retain(known);
        let recents = home.as_deref().map(recent::load).unwrap_or_default();
        for row in recents.into_iter().filter(known).take(5) {
            add(&mut rows, row);
        }
        let top = rows.len();
        let mut facts = BTreeMap::new();
        let mut groups = BTreeMap::new();
        let mut causes = BTreeMap::new();
        let kept = crate::catalog::cached();
        let checked = |name: &str| only.as_deref().is_none_or(|only| only == name);
        for endpoint in &endpoints {
            let listing = kept.get(&endpoint.name);
            if checked(&endpoint.name) {
                let state = if listing.is_some() {
                    State::Checking
                } else {
                    State::Loading
                };
                groups.insert(endpoint.name.clone(), state);
            }
            if !endpoint.model.is_empty() {
                add(&mut rows, Seat::new(&endpoint.name, &endpoint.model));
            }
            for model in listing.into_iter().flat_map(|listing| &listing.models) {
                let row = Seat::new(&endpoint.name, &model.id);
                facts.insert(row.clone(), model.clone());
                add(&mut rows, row);
            }
        }
        for (name, model, state) in &setup {
            groups.insert(name.clone(), *state);
            add(&mut rows, Seat::new(name, model));
        }
        let mut registry = Some(tokio::spawn(crate::catalog::refresh_registry()));
        let mut pending: FuturesUnordered<_> = endpoints
            .iter()
            .filter(|endpoint| checked(&endpoint.name))
            .map(
                |endpoint| async move { (endpoint.name.clone(), Client::discover(endpoint).await) },
            )
            .collect();
        loop {
            order(&mut rows[top..], &facts, &groups);
            let mut sent: Vec<Seat> = home.as_deref().map(recent::pinned).unwrap_or_default();
            sent.retain(|row| rows.contains(row));
            let pinned = sent.len();
            for row in &rows[..top] {
                add(&mut sent, row.clone());
            }
            let head = sent.len();
            for row in &rows[top..] {
                add(&mut sent, row.clone());
            }
            self.send(Msg::Models {
                request,
                pinned,
                recent: head - pinned,
                rows: sent,
                facts: facts.clone(),
                groups: groups.clone(),
                causes: causes.clone(),
                loading: !pending.is_empty(),
                error: errors.join("; "),
            });
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
                        enrich(&endpoints, &mut facts);
                    }
                    continue;
                }
            };
            match result {
                Ok(models) => {
                    let fresh: Vec<Seat> = models
                        .iter()
                        .map(|model| Seat::new(&name, &model.id))
                        .collect();
                    let gone = |row: &Seat| {
                        row.endpoint == name && facts.contains_key(row) && !fresh.contains(row)
                    };
                    let tail: Vec<Seat> = rows.split_off(top);
                    rows.extend(tail.into_iter().filter(|row| !gone(row)));
                    facts.retain(|row, _| row.endpoint != name || fresh.contains(row));
                    for (row, model) in fresh.into_iter().zip(models) {
                        facts.insert(row.clone(), model);
                        add(&mut rows, row);
                    }
                    causes.remove(&name);
                    groups.insert(name, State::Ready);
                }
                Err(error) => {
                    groups.insert(name.clone(), Client::state(&error));
                    causes.insert(name, error.to_string());
                }
            }
        }
    }
}
