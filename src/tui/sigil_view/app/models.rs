use sigil::widgets::CursorEdge;

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

impl App {
    pub(in crate::tui::sigil_view) fn begin_model_edit(&self) {
        self.picker.choices.set(Rc::default());
        self.replace_model_draft(String::new());
        self.picker.focus.set(self.picker.focus.peek() + 1);
        self.picker.groups.set(std::collections::BTreeMap::new());
        self.picker.causes.set(std::collections::BTreeMap::new());
        self.picker
            .default
            .set(crate::endpoint::default_model_selection());
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

    pub(in crate::tui::sigil_view) fn picked_model(&self) -> Option<ModelId> {
        let shown = self.picker.shown.peek();
        shown.get(self.picker.list.at(shown.len())).cloned()
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
        let selected = current > 0 && rows.first() == Some(&row);
        let pinned = if selected {
            !summonr_core::dirs::summonr_home()
                .is_ok_and(|home| crate::recent::pinned(&home).contains(&row))
        } else {
            !at.is_some_and(|at| at < current + pins)
        };
        if !selected {
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
        batch(|| {
            self.picker.recent.set([current, pins, recent]);
            self.picker.choices.set(Rc::new(rows));
        });
        if let Some(at) = self
            .picker
            .shown
            .peek()
            .iter()
            .position(|held| *held == row)
        {
            self.picker.list.cursor.set(at);
        }
    }

    pub(in crate::tui::sigil_view) fn label(&self, row: &ModelId) -> String {
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

    pub(in crate::tui::sigil_view) fn defaulted(&self, selection: Option<ModelId>) {
        let note = match (&selection, self.picker.default.peek()) {
            (Some(selection), _) => {
                format!("◆ default {} · summonr starts on it", self.label(selection))
            }
            (None, Some(old)) => format!(
                "cleared the default; {} is no longer the default model",
                self.label(&old)
            ),
            (None, None) => "cleared the default".to_string(),
        };
        self.picker.default.set(selection);
        self.picker.note.set(note);
    }

    pub(in crate::tui::sigil_view) fn replace_model_draft(&self, draft: String) {
        if self.picker.list.query.peek() != draft {
            self.picker.list.query.set(draft);
        }
        self.picker.note.set(String::new());
        self.picker.list.cursor.set(0);
    }

    pub(in crate::tui::sigil_view) fn commit_model_edit(&self) {
        if self.auth.open.peek() {
            return;
        }
        let draft = self.picker.list.query.peek().trim().to_string();
        let current = self.model.peek().snapshot.endpoint.clone();
        let groups = self.picker.groups.peek();
        let picked = match self.picked_model() {
            Some(picked) => picked,
            None if draft.is_empty() || self.picker.loading.peek() => return,
            None => draft
                .split_once('/')
                .filter(|(name, _)| groups.contains_key(*name))
                .map_or_else(
                    || ModelId::new(current.clone(), draft.clone()),
                    |(name, id)| ModelId::new(name, id),
                ),
        };
        if self.unblock(&groups, &picked) {
            return;
        }
        let target = self.picker.target.peek();
        self.cancel_model_edit();
        let ModelId { endpoint, model } = picked;
        drop(
            self.cmd
                .send(match target.and_then(|index| Role::ALL.get(index)) {
                    Some(role) => Cmd::AssignModel(*role, endpoint, model),
                    None if endpoint == current => Cmd::SetModel(model),
                    None => Cmd::SelectModel(endpoint, model),
                }),
        );
    }

    fn unblock(
        &self,
        groups: &std::collections::BTreeMap<String, State>,
        picked: &ModelId,
    ) -> bool {
        let Some(state) = groups.get(&picked.endpoint).copied() else {
            return false;
        };
        if state.setup() || state == State::KeyRejected {
            self.begin_auth(Some(&picked.endpoint));
            self.auth
                .selection
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
        let at = self.picker.list.at(shown.len());
        self.picker.list.query.peek().trim().is_empty()
            && self.picker.facet.peek() == super::facet::Facet::All
            && at >= listed
            && shown.get(at).is_some_and(|selection| {
                self.picker
                    .groups
                    .peek()
                    .get(&selection.endpoint)
                    .is_some_and(|state| state.blocked())
            })
    }

    pub(in crate::tui::sigil_view) fn favorite(&self, selection: &ModelId) -> bool {
        let [current, pins, _] = self.picker.recent.get();
        self.picker
            .choices
            .get()
            .iter()
            .skip(current)
            .take(pins)
            .any(|pin| pin == selection)
    }

    pub(in crate::tui::sigil_view) fn mark(&self, selection: &ModelId) -> Mark {
        let [current, _, _] = self.picker.recent.get();
        let choices = self.picker.choices.get();
        if current > 0 && choices.first() == Some(selection) {
            Mark::Current
        } else if self.picker.default.get().as_ref() == Some(selection) {
            Mark::Default
        } else if self.favorite(selection) {
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
        self.picker
            .list
            .shift(delta, self.picker.shown.peek().len(), CursorEdge::Clamp);
    }

    pub(in crate::tui::sigil_view) fn toggle_older_models(&self) {
        let held = self.picked_model();
        self.picker.older.set(!self.picker.older.peek());
        let at = held
            .and_then(|held| {
                self.picker
                    .shown
                    .peek()
                    .iter()
                    .position(|selection| *selection == held)
            })
            .unwrap_or(0);
        self.picker.list.cursor.set(at);
    }
}
