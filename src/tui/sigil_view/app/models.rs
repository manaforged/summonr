use sigil::prelude::fuzzy_score;

use super::*;

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
        self.model_choices.set(Rc::default());
        self.replace_model_draft(String::new());
        self.models_focus.set(self.models_focus.peek() + 1);
        self.model_groups.set(std::collections::BTreeMap::new());
        self.model_causes.set(std::collections::BTreeMap::new());
        self.model_default.set(crate::endpoint::default_seat());
        self.model_recent.set([0; 3]);
        self.editing_model.set(true);
        self.list_models(None);
    }

    pub(in crate::tui::sigil_view) fn refresh_models(&self) {
        let only = self
            .picked_model()
            .map(|row| row.endpoint)
            .filter(|name| self.model_causes.peek().contains_key(name));
        self.list_models(only);
    }

    pub(in crate::tui::sigil_view) fn list_models(&self, only: Option<String>) {
        self.model_error.set(String::new());
        self.model_loading.set(true);
        let request = self.model_request.get().wrapping_add(1);
        self.model_request.set(request);
        let (sender, cancel) = oneshot::channel();
        *self.model_cancel.borrow_mut() = Some(sender);
        if self
            .cmd
            .send(Cmd::ListModels {
                request,
                only,
                cancel,
            })
            .is_err()
        {
            self.model_loading.set(false);
            self.model_error
                .set("Model discovery is unavailable.".to_string());
        }
    }

    pub(in crate::tui::sigil_view) fn picked_model(&self) -> Option<Seat> {
        let shown = self.model_shown.peek();
        shown
            .get(self.model_sel.peek().min(shown.len().saturating_sub(1)))
            .cloned()
    }

    pub(in crate::tui::sigil_view) fn pin_model(&self) {
        let Some(row) = self.picked_model() else {
            return;
        };
        let [current, mut pins, mut recent] = self.model_recent.peek();
        let mut rows = Vec::clone(&self.model_choices.peek());
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
        self.model_note.set(if pinned {
            format!("★ pinned {name}")
        } else {
            format!("unpinned {name}")
        });
        self.model_recent.set([current, pins, recent]);
        self.model_choices.set(Rc::new(rows));
        self.rank();
        if let Some(at) = self.model_shown.peek().iter().position(|held| *held == row) {
            self.model_sel.set(at);
        }
    }

    pub(in crate::tui::sigil_view) fn label(&self, row: &Seat) -> String {
        self.model_facts
            .peek()
            .get(row)
            .map_or_else(|| row.model.clone(), |fact| fact.label().to_string())
    }

    pub(in crate::tui::sigil_view) fn default_model(&self) {
        let Some(row) = self.picked_model() else {
            return;
        };
        let clear = self.model_default.peek().as_ref() == Some(&row);
        drop(self.cmd.send(Cmd::SaveDefault((!clear).then_some(row))));
    }

    pub(in crate::tui::sigil_view) fn defaulted(&self, seat: Option<Seat>) {
        let note = match (&seat, self.model_default.peek()) {
            (Some(seat), _) => format!("◆ default {} · summonr starts on it", self.label(seat)),
            (None, Some(old)) => format!(
                "cleared the default; {} is no longer the start seat",
                self.label(&old)
            ),
            (None, None) => "cleared the default".to_string(),
        };
        self.model_default.set(seat);
        self.model_note.set(note);
    }

    pub(in crate::tui::sigil_view) fn replace_model_draft(&self, draft: String) {
        if self.model_draft.peek() != draft {
            self.model_draft.set(draft);
        }
        self.rank();
        self.model_note.set(String::new());
        self.model_sel.set(0);
    }

    pub(in crate::tui::sigil_view) fn commit_model_edit(&self) {
        let draft = self.model_draft.peek().trim().to_string();
        let current = self.model.peek().snapshot.endpoint.clone();
        let groups = self.model_groups.peek();
        let picked = match self.picked_model() {
            Some(picked) => picked,
            None if draft.is_empty() || self.model_loading.peek() => return,
            None => draft
                .split_once('/')
                .filter(|(name, _)| groups.contains_key(*name))
                .map_or_else(
                    || Seat::new(current.clone(), draft.clone()),
                    |(name, id)| Seat::new(name, id),
                ),
        };
        if matches!(
            groups.get(&picked.endpoint),
            Some(state) if state.setup() || *state == State::KeyRejected
        ) {
            self.begin_auth(Some(&picked.endpoint));
            self.auth_seat.set(Some((picked.endpoint, picked.model)));
            return;
        }
        let target = self.model_for.peek();
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

    pub(in crate::tui::sigil_view) fn cancel_model_edit(&self) {
        drop(self.model_cancel.borrow_mut().take());
        self.editing_model.set(false);
        self.model_loading.set(false);
        self.model_for.set(None);
        self.replace_model_draft(String::new());
    }

    pub(in crate::tui::sigil_view) fn move_model(&self, delta: isize) {
        self.model_note.set(String::new());
        let last = self.model_shown.peek().len().saturating_sub(1);
        self.model_sel
            .set(self.model_sel.peek().saturating_add_signed(delta).min(last));
    }

    pub(in crate::tui::sigil_view) fn rank(&self) {
        let query = self.model_draft.peek().to_lowercase();
        let choices = self.model_choices.peek();
        if query.trim().is_empty() {
            self.model_shown.set(choices);
            return;
        }
        let words: Vec<&str> = query.split_whitespace().collect();
        let facts = self.model_facts.peek();
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
        self.model_shown.set(Rc::new(
            ranked.into_iter().map(|(_, row)| row.clone()).collect(),
        ));
    }
}
