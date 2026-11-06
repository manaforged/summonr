use sigil::prelude::ListRowSlots;
use sigil::prelude::NodeBuilder;
use sigil::prelude::Token;
use sigil::widgets::ListColumn;
use sigil::widgets::RowTone;

use super::super::canvas;
use crate::catalog::{Cost, Model};
use crate::endpoint::State;
use crate::recent::ModelId;
use crate::tui::sigil_view::text_style::TextStyle;
use crate::tui::sigil_view::ui::{heading, slotted};

const NAME_MIN: usize = 16;
const NAME_MAX: usize = 44;
const NAME_SPLIT: usize = 34;
const PROVIDER: f32 = 14.0;
const CONTEXT: f32 = 9.0;
const PRICE: f32 = 16.0;
const RELEASED: f32 = 12.0;
const CAN: f32 = 14.0;
const MARGIN: usize = 6;

#[derive(Clone, Copy)]
pub(super) struct Grid {
    name: usize,
    provider: bool,
    released: bool,
    can: bool,
}

impl Grid {
    pub(super) fn new(span: usize, split: bool, longest: usize, varied: bool) -> Self {
        let most = if split { NAME_SPLIT } else { NAME_MAX };
        let wanted = longest.clamp(NAME_MIN, most);
        let room = span.saturating_sub(MARGIN);
        let fit = |provider: bool| {
            let grid = Self {
                name: wanted,
                provider,
                released: false,
                can: false,
            };
            Self {
                name: wanted
                    .min(room.saturating_sub(grid.cells() as usize - wanted))
                    .max(NAME_MIN),
                ..grid
            }
        };
        let mut grid = fit(true);
        if grid.name < wanted {
            grid = fit(false);
        }
        if split {
            return grid;
        }
        let adds: [fn(&mut Self); 2] = [|grid| grid.released = true, |grid| grid.can = true];
        for add in &adds[..if varied { 2 } else { 1 }] {
            let mut wider = grid;
            add(&mut wider);
            if grid.name == wanted && wider.cells() as usize <= room {
                grid = wider;
            }
        }
        if canvas::is_window() {
            grid.name += room.saturating_sub(grid.cells() as usize);
        }
        grid
    }

    pub(super) fn cells(self) -> f32 {
        let columns = CONTEXT
            + PRICE
            + if self.provider { PROVIDER } else { 0.0 }
            + if self.released { RELEASED } else { 0.0 }
            + if self.can { CAN } else { 0.0 };
        let count =
            2 + usize::from(self.provider) + usize::from(self.released) + usize::from(self.can);
        self.name as f32 + columns + 8.0 + count as f32
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
        if self.provider {
            columns.push(ListColumn::new(provider, width(PROVIDER)).start());
        }
        let columns = columns
            .into_iter()
            .flat_map(|column| [ListColumn::new(String::new(), 0.0), column])
            .collect();
        ListRowSlots {
            columns,
            title_width: Some(width(self.name as f32)),
            tone,
            ..ListRowSlots::default()
        }
    }

    pub(super) fn header(self) -> NodeBuilder {
        let labels = ["CONTEXT", "$/1M IN/OUT", "RELEASED", "CAN", "PROVIDER"].map(str::to_string);
        slotted(
            false,
            Some(" "),
            "MODEL",
            self.slots(labels, RowTone::Header),
        )
    }

    pub(super) fn row(
        self,
        selection: &ModelId,
        fact: Option<&Model>,
        shown: Shown,
    ) -> ListRowSlots {
        let provider = if shown.provider {
            selection.endpoint.clone()
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
        let price = price(fact);
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
            (Some(state), _) if state.blocked() => state.word().to_string(),
            (Some(State::Ready), None) => "not listed".to_string(),
            (_, Some(fact)) => abilities(fact),
            (_, None) => String::new(),
        };
        let tone = if shown.state.is_some_and(State::blocked) {
            RowTone::Muted
        } else {
            RowTone::Normal
        };
        self.slots([context, price, released, can, provider], tone)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Shown {
    pub(super) provider: bool,
    pub(super) state: Option<State>,
}

pub(in crate::tui::sigil_view) fn price(fact: Option<&Model>) -> String {
    match fact.map(|fact| (fact.cost, fact.price)) {
        Some((Cost::Free, _)) => "free".to_string(),
        Some((Cost::Plan, _)) => "plan".to_string(),
        Some((Cost::Metered, Some(price))) => format!(
            "{}/{}",
            crate::provider::dollars(price.input),
            crate::provider::dollars(price.output)
        ),
        Some((Cost::Metered, None)) => "paid".to_string(),
        Some((Cost::Unknown, _)) | None => "—".to_string(),
    }
}

pub(in crate::tui::sigil_view) fn tone(state: State) -> Token {
    match state {
        State::Ready | State::NoKey => Token::Success11,
        State::Checking | State::Loading => Token::Gray10,
        State::RateLimited | State::SignIn | State::NeedsKey | State::NeedsModel => Token::Warn11,
        State::KeyRejected | State::Offline | State::Error => Token::Error11,
    }
}

pub(super) fn abilities(fact: &Model) -> String {
    [(fact.reasoning, "think"), (fact.vision, "vision")]
        .into_iter()
        .filter_map(|(on, word)| on.then_some(word))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn section_heading(title: &str) -> NodeBuilder {
    heading(vec![(title.to_string(), TextStyle::xs(Token::Gray11))])
}

pub(super) fn provider_heading(name: &str, state: Option<State>, count: usize) -> NodeBuilder {
    let mut parts = vec![
        (name.to_string(), TextStyle::xs(Token::Gray11)),
        (format!("  {count}"), TextStyle::xs(Token::Gray10)),
    ];
    if let Some(state) = state.filter(|state| *state != State::Ready) {
        parts.push((format!(" · {}", state.word()), TextStyle::xs(tone(state))));
    }
    heading(parts)
}

pub(super) fn detail(selection: &ModelId, fact: Option<&Model>, state: Option<State>) -> String {
    let mut parts = vec![format!("{}/{}", selection.endpoint, selection.model)];
    if let Some(fact) = fact {
        if let Some(day) = fact.released_on() {
            parts.push(format!("released {day}"));
        }
        let can = abilities(fact);
        if !can.is_empty() {
            parts.push(can);
        }
        if let Some(price) = fact.price.filter(|_| fact.cost == Cost::Metered) {
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
            selection.endpoint
        ));
    }
    parts.join(" · ")
}
