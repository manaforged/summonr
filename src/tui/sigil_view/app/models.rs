use sigil::prelude::fuzzy_score;

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Mark {
    Current,
    Default,
    Favorite,
    None,
}

impl Mark {
    pub(in crate::tui::sigil_view) const fn glyph(self) -> &'static str {
        match self {
            Self::Current => "●",
            Self::Default => "◆",
            Self::Favorite => "★",
            Self::None => " ",
        }
    }

    pub(in crate::tui::sigil_view) const fn word(self) -> &'static str {
        match self {
            Self::Current => "● current",
            Self::Default => "◆ default",
            Self::Favorite => "★ favorite",
            Self::None => "",
        }
    }
}

fn score(query: &str, row: &Seat, display: &str) -> Option<usize> {
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

impl App {
    pub(in crate::tui::sigil_view) fn begin_model_edit(&self) {
        self.picker.choices.set(Rc::default());
        self.replace_model_draft(String::new());
        self.picker.focus.set(self.picker.focus.peek() + 1);
        self.picker.groups.set(std::collections::BTreeMap::new());
        self.picker.causes.set(std::collections::BTreeMap::new());
        self.picker.default.set(crate::endpoint::default_seat());
        self.picker.recent.set([0; 3]);
        self.picker.older.set(false);
        self.picker.facet.set(super::facet::Facet::All);
        self.picker.open.set(true);
        self.list_models(None);
    }

    pub(in crate::tui::sigil_view) fn refresh_models(&self) {
        let only = self
            .picked_model()
            .map(|row| row.endpoint)
            .filter(|name| self.picker.causes.peek().contains_key(name));
        self.list_models(only);
    }

    pub(in crate::tui::sigil_view) fn list_models(&self, only: Option<String>) {
        self.picker.error.set(String::new());
        self.picker.loading.set(true);
        let request = self.picker.request.get().wrapping_add(1);
        self.picker.request.set(request);
        let (sender, cancel) = oneshot::channel();
        *self.picker.cancel.borrow_mut() = Some(sender);
        if self
            .cmd
            .send(Cmd::ListModels {
                request,
                only,
                cancel,
            })
            .is_err()
        {
            self.picker.loading.set(false);
            self.picker
                .error
                .set("Model discovery is unavailable.".to_string());
        }
    }

    pub(in crate::tui::sigil_view) fn picked_model(&self) -> Option<Seat> {
        let shown = self.picker.shown.peek();
        shown
            .get(self.picker.sel.peek().min(shown.len().saturating_sub(1)))
            .cloned()
    }

    pub(in crate::tui::sigil_view) fn pin_model(&self) {
        let Some(row) = self.picked_model().filter(|_| !self.fixing()) else {
            return;
        };
        let [current, mut pins, mut recent] = self.picker.recent.peek();
        let mut rows = Vec::clone(&self.picker.choices.peek());
        let at = rows
            .iter()
            .skip(current)
            .position(|held| *held == row)
            .map(|at| at + current);
        let seated = current > 0 && rows.first() == Some(&row);
        let pinned = if seated {
            !crate::prompt::summonr_home()
                .is_ok_and(|home| crate::recent::pinned(&home).contains(&row))
        } else {
            !at.is_some_and(|at| at < current + pins)
        };
        if !seated {
            if let Some(at) = at {
                rows.remove(at);
                if at < current + pins {
                    pins -= 1;
                } else if at < current + pins + recent {
                    recent -= 1;
                }
            }
            rows.insert(current + pins, row.clone());
            if pinned {
                pins += 1;
            } else {
                recent += 1;
            }
        }
        drop(self.cmd.send(Cmd::Pin(row.clone(), pinned)));
        let name = self.label(&row);
        self.picker.note.set(if pinned {
            format!("★ favorite {name}")
        } else {
            format!("removed {name} from favorites")
        });
        self.picker.recent.set([current, pins, recent]);
        self.picker.choices.set(Rc::new(rows));
        self.rank();
        if let Some(at) = self
            .picker
            .shown
            .peek()
            .iter()
            .position(|held| *held == row)
        {
            self.picker.sel.set(at);
        }
    }

    pub(in crate::tui::sigil_view) fn label(&self, row: &Seat) -> String {
        self.picker
            .facts
            .peek()
            .get(row)
            .map_or_else(|| row.model.clone(), |fact| fact.label().to_string())
    }

    pub(in crate::tui::sigil_view) fn default_model(&self) {
        let Some(row) = self.picked_model().filter(|_| !self.fixing()) else {
            return;
        };
        let clear = self.picker.default.peek().as_ref() == Some(&row);
        drop(self.cmd.send(Cmd::SaveDefault((!clear).then_some(row))));
    }

    pub(in crate::tui::sigil_view) fn defaulted(&self, seat: Option<Seat>) {
        let note = match (&seat, self.picker.default.peek()) {
            (Some(seat), _) => format!("◆ default {} · summonr starts on it", self.label(seat)),
            (None, Some(old)) => format!(
                "cleared the default; {} is no longer the start seat",
                self.label(&old)
            ),
            (None, None) => "cleared the default".to_string(),
        };
        self.picker.default.set(seat);
        self.picker.note.set(note);
    }

    pub(in crate::tui::sigil_view) fn replace_model_draft(&self, draft: String) {
        if self.picker.draft.peek() != draft {
            self.picker.draft.set(draft);
        }
        self.rank();
        self.picker.note.set(String::new());
        self.picker.sel.set(0);
    }

    pub(in crate::tui::sigil_view) fn commit_model_edit(&self) {
        let draft = self.picker.draft.peek().trim().to_string();
        let current = self.model.peek().snapshot.endpoint.clone();
        let groups = self.picker.groups.peek();
        let picked = match self.picked_model() {
            Some(picked) => picked,
            None if draft.is_empty() || self.picker.loading.peek() => return,
            None => draft
                .split_once('/')
                .filter(|(name, _)| groups.contains_key(*name))
                .map_or_else(
                    || Seat::new(current.clone(), draft.clone()),
                    |(name, id)| Seat::new(name, id),
                ),
        };
        if self.unblock(&groups, &picked) {
            return;
        }
        let target = self.picker.target.peek();
        self.cancel_model_edit();
        let Seat { endpoint, model } = picked;
        drop(
            self.cmd
                .send(match target.and_then(|index| Role::ALL.get(index)) {
                    Some(role) => Cmd::SeatRole(*role, endpoint, model),
                    None if endpoint == current => Cmd::SetModel(model),
                    None => Cmd::SetSeat(endpoint, model),
                }),
        );
    }

    fn unblock(&self, groups: &std::collections::BTreeMap<String, State>, picked: &Seat) -> bool {
        let Some(state) = groups.get(&picked.endpoint).copied() else {
            return false;
        };
        if state.setup() || state == State::KeyRejected {
            self.begin_auth(Some(&picked.endpoint));
            self.auth
                .seat
                .set(Some((picked.endpoint.clone(), picked.model.clone())));
            return true;
        }
        if self.fixing() {
            self.list_models(Some(picked.endpoint.clone()));
            return true;
        }
        false
    }

    pub(in crate::tui::sigil_view) fn fixing(&self) -> bool {
        let listed: usize = self.picker.recent.peek().iter().sum();
        let shown = self.picker.shown.peek();
        let at = self.picker.sel.peek().min(shown.len().saturating_sub(1));
        self.picker.draft.peek().trim().is_empty()
            && self.picker.facet.peek() == super::facet::Facet::All
            && at >= listed
            && shown.get(at).is_some_and(|seat| {
                self.picker
                    .groups
                    .peek()
                    .get(&seat.endpoint)
                    .is_some_and(|state| state.blocked())
            })
    }

    pub(in crate::tui::sigil_view) fn favorite(&self, seat: &Seat) -> bool {
        let [current, pins, _] = self.picker.recent.get();
        self.picker
            .choices
            .get()
            .iter()
            .skip(current)
            .take(pins)
            .any(|pin| pin == seat)
    }

    pub(in crate::tui::sigil_view) fn mark(&self, seat: &Seat) -> Mark {
        let [current, _, _] = self.picker.recent.get();
        let choices = self.picker.choices.get();
        if current > 0 && choices.first() == Some(seat) {
            Mark::Current
        } else if self.picker.default.get().as_ref() == Some(seat) {
            Mark::Default
        } else if self.favorite(seat) {
            Mark::Favorite
        } else {
            Mark::None
        }
    }

    pub(in crate::tui::sigil_view) fn cancel_model_edit(&self) {
        drop(self.picker.cancel.borrow_mut().take());
        self.picker.open.set(false);
        self.picker.loading.set(false);
        self.picker.target.set(None);
        self.replace_model_draft(String::new());
    }

    pub(in crate::tui::sigil_view) fn move_model(&self, delta: isize) {
        self.picker.note.set(String::new());
        let last = self.picker.shown.peek().len().saturating_sub(1);
        self.picker.sel.set(
            self.picker
                .sel
                .peek()
                .saturating_add_signed(delta)
                .min(last),
        );
    }

    pub(super) fn picker_rows(&self, older: bool, collapse: bool) -> (Vec<Seat>, usize) {
        let listed: usize = self.picker.recent.peek().iter().sum();
        let facts = self.picker.facts.peek();
        let choices = self.picker.choices.peek();
        let (held, rest) = choices.split_at(listed.min(choices.len()));
        let served: Vec<(&Seat, Option<&crate::catalog::Model>)> = rest
            .iter()
            .map(|seat| (seat, facts.get(seat)))
            .filter(|(_, fact)| fact.is_none_or(|fact| fact.usable()))
            .collect();
        let shelf: Vec<(&str, &crate::catalog::Model)> = served
            .iter()
            .filter_map(|(seat, fact)| Some((seat.endpoint.as_str(), (*fact)?)))
            .collect();
        let mut current = crate::catalog::current(&shelf).into_iter();
        let groups = self.picker.groups.peek();
        let mut fixes = std::collections::BTreeSet::new();
        let mut hidden = 0;
        let mut kept = Vec::new();
        for (seat, fact) in &served {
            let now = fact.is_none() || current.next().unwrap_or(true);
            if collapse
                && groups
                    .get(&seat.endpoint)
                    .is_some_and(|state| state.blocked())
            {
                if fixes.insert(seat.endpoint.as_str()) {
                    kept.push((*seat).clone());
                }
                continue;
            }
            hidden += usize::from(!now);
            if older || now {
                kept.push((*seat).clone());
            }
        }
        (held.iter().cloned().chain(kept).collect(), hidden)
    }

    pub(in crate::tui::sigil_view) fn toggle_older_models(&self) {
        let held = self.picked_model();
        self.picker.older.set(!self.picker.older.peek());
        self.rank();
        let at = held
            .and_then(|held| {
                self.picker
                    .shown
                    .peek()
                    .iter()
                    .position(|seat| *seat == held)
            })
            .unwrap_or(0);
        self.picker.sel.set(at);
    }

    pub(in crate::tui::sigil_view) fn rank(&self) {
        let (rows, hidden) = self.ranked(self.picker.facet.peek());
        self.picker.hidden.set(hidden);
        self.picker.shown.set(Rc::new(rows));
    }

    pub(super) fn ranked(&self, facet: super::facet::Facet) -> (Vec<Seat>, usize) {
        let query = self.picker.draft.peek().to_lowercase();
        if query.trim().is_empty() {
            let collapse = facet == super::facet::Facet::All;
            let (rows, hidden) = self.picker_rows(self.picker.older.peek(), collapse);
            return (self.faceted(rows, facet), hidden);
        }
        let (choices, _) = self.picker_rows(true, false);
        let choices = self.faceted(choices, facet);
        let words: Vec<&str> = query.split_whitespace().collect();
        let facts = self.picker.facts.peek();
        let mut ranked: Vec<(usize, &Seat)> = choices
            .iter()
            .filter_map(|row| {
                let display = facts.get(row).map_or("", |fact| fact.label());
                let rank = words
                    .iter()
                    .map(|word| score(word, row, display))
                    .try_fold(0, |worst, rank| Some(worst.max(rank?)))?;
                Some((rank, row))
            })
            .collect();
        ranked.sort_by_key(|(rank, _)| *rank);
        (ranked.into_iter().map(|(_, row)| row.clone()).collect(), 0)
    }
}
