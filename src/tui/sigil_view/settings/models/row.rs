use sigil::prelude::ListRowSlots;
use sigil::prelude::NodeBuilder;
use sigil::prelude::Token;
use sigil::widgets::ListColumn;
use sigil::widgets::RowTone;

use super::super::canvas;
use crate::catalog::{Cost, Model};
use crate::endpoint::State;
use crate::recent::Seat;
use crate::tui::sigil_view::look::Look;
use crate::tui::sigil_view::ui::{heading, slotted};

const NAME_MIN: usize = 16;
const NAME_MAX: usize = 44;
const PROVIDER: f32 = 14.0;
const CONTEXT: f32 = 7.0;
const PRICE: f32 = 5.0;
const RELEASED: f32 = 11.0;
const CAN: f32 = 14.0;

#[derive(Clone, Copy)]
pub(super) struct Grid {
    name: usize,
    released: bool,
    can: bool,
}

impl Grid {
    pub(super) fn new(span: usize, longest: usize) -> Self {
        Self {
            name: (longest + 2).clamp(NAME_MIN, NAME_MAX),
            released: span >= 64,
            can: span >= 80,
        }
    }

    fn slots(self, cells: [String; 5], tone: RowTone) -> ListRowSlots {
        let [context, price, released, can, provider] = cells;
        let width = |cells: f32| cells * canvas::cell().w;
        let mut columns = vec![
            ListColumn::new(context, width(CONTEXT)),
            ListColumn::new(price, width(PRICE)).start(),
        ];
        if self.released {
            columns.push(ListColumn::new(released, width(RELEASED)).start());
        }
        if self.can {
            columns.push(ListColumn::new(can, width(CAN)).start());
        }
        columns.push(ListColumn::new(provider, width(PROVIDER)).start());
        ListRowSlots {
            columns,
            title_width: Some(width(self.name as f32)),
            tone,
            ..ListRowSlots::default()
        }
    }

    pub(super) fn header(self) -> NodeBuilder {
        let labels = ["CONTEXT", "PRICE", "RELEASED", "CAN", "PROVIDER"].map(str::to_string);
        slotted(
            false,
            Some(" "),
            "MODEL",
            self.slots(labels, RowTone::Header),
        )
    }

    pub(super) fn row(self, seat: &Seat, fact: Option<&Model>, shown: Shown) -> ListRowSlots {
        let provider = if shown.provider {
            seat.endpoint.clone()
        } else {
            String::new()
        };
        let dash = |text: String| {
            if text.is_empty() {
                "—".to_string()
            } else {
                text
            }
        };
        let context = dash(fact.map(Model::window).unwrap_or_default());
        let price = match fact.map_or(Cost::Unknown, |fact| fact.cost) {
            Cost::Free => "free",
            Cost::Plan => "plan",
            Cost::Metered => "paid",
            Cost::Unknown => "—",
        };
        let released = dash(
            fact.and_then(Model::released)
                .map_or_else(String::new, |month| {
                    if fact.is_some_and(Model::fresh) {
                        format!("{month} new")
                    } else {
                        month
                    }
                }),
        );
        let can = match (shown.state, fact) {
            (Some(state), _) if blocked(state) => state.word().to_string(),
            (Some(State::Ready), None) => "not listed".to_string(),
            (_, Some(fact)) => [(fact.reasoning, "think"), (fact.vision, "vision")]
                .into_iter()
                .filter_map(|(on, word)| on.then_some(word))
                .collect::<Vec<_>>()
                .join(" "),
            (_, None) => String::new(),
        };
        let tone = if shown.state.is_some_and(blocked) {
            RowTone::Muted
        } else {
            RowTone::Normal
        };
        self.slots([context, price.to_string(), released, can, provider], tone)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Shown {
    pub(super) provider: bool,
    pub(super) state: Option<State>,
}

pub(super) const fn blocked(state: State) -> bool {
    matches!(
        state,
        State::SignIn
            | State::NeedsKey
            | State::NeedsModel
            | State::KeyRejected
            | State::Offline
            | State::Error
    )
}

pub(super) fn section_heading(title: &str) -> NodeBuilder {
    heading(vec![(title.to_string(), Look::xs(Token::Gray12).bold())])
}

pub(super) fn provider_heading(name: &str, state: Option<State>, count: usize) -> NodeBuilder {
    let mut parts = vec![(name.to_string(), Look::xs(Token::Gray12).bold())];
    if let Some(state) = state {
        let tone = match state {
            State::Ready | State::NoKey => Look::xs(Token::Success11),
            State::Checking | State::Loading => Look::xs(Token::Gray10),
            State::RateLimited | State::SignIn | State::NeedsKey | State::NeedsModel => {
                Look::xs(Token::Warn11)
            }
            State::KeyRejected | State::Offline | State::Error => Look::xs(Token::Error11),
        };
        parts.push((format!(" · {}", state.word()), tone));
    }
    parts.push((format!(" · {count}"), Look::xs(Token::Gray10)));
    heading(parts)
}

pub(super) fn detail(seat: &Seat, fact: Option<&Model>, state: Option<State>) -> String {
    let mut parts = vec![format!("{}/{}", seat.endpoint, seat.model)];
    if let Some(fact) = fact {
        if let Some(day) = fact.released_on() {
            parts.push(format!("released {day}"));
        }
        if let Some(price) = fact.price {
            parts.push(format!(
                "{} in · {} out per 1M tokens",
                crate::provider::dollars(price.input),
                crate::provider::dollars(price.output)
            ));
        }
        if let Some(about) = fact.about.as_deref() {
            parts.push(about.to_string());
        }
    } else if state == Some(State::Ready) {
        parts.push(format!(
            "{} does not list this model; ↵ still tries it",
            seat.endpoint
        ));
    }
    parts.join(" · ")
}
