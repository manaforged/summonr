use sigil::prelude::AlignItems;

use sigil::prelude::NodeBuilder;
use sigil::prelude::Space;
use sigil::prelude::spacer;

use super::{App, cell_width};
use crate::text_style::TextStyle;
use sigil::prelude::{Token, Widget};

mod meter;
use meter::{meter, meter_button};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Context,
    Cache,
}

type FooterView = (
    Option<super::super::app::Notice>,
    Option<String>,
    Option<(String, bool)>,
    Option<u64>,
    Option<u64>,
    Option<(bool, Vec<(String, String)>)>,
);

fn footer_view(app: &App) -> FooterView {
    let context_open = app.context_open.get();
    let cache_open = app.cache_open.get();
    let model = app.model.get();
    let spend = model.spend().map(summonr_agent::dollars);
    let started = !model.usage.is_empty() || model.entries.asked();
    let open = if !started {
        None
    } else if context_open {
        Some((false, model.context_facts()))
    } else if cache_open {
        Some((true, model.cache_facts()))
    } else {
        None
    };
    (
        app.notice.get(),
        spend,
        elsewhere(&model.live),
        started.then(|| model.context_percent()).flatten(),
        started.then(|| model.cache_percent()),
        open,
    )
}

fn elsewhere(live: &[summonr_agent::hub::Live]) -> Option<(String, bool)> {
    let others: Vec<_> = live.iter().filter(|entry| !entry.active).collect();
    if others.is_empty() {
        return None;
    }
    let waiting = others
        .iter()
        .filter(|entry| entry.status == summonr_agent::hub::Status::NeedsYou)
        .count();
    let open = summonr_core::text::plural(others.len(), "other session");
    Some(if waiting == 0 {
        (open, false)
    } else {
        (format!("{open} · {waiting} waiting on you · /resume"), true)
    })
}

pub(super) fn footer_bar(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let view = sigil::reactive::derived(move || footer_view(&update));
    let place_app = app.clone();
    let place =
        sigil::reactive::derived(move || view.with(|view| place_of(&place_app, view, width)));
    let shape = sigil::reactive::derived(move || {
        let (notice, spend, elsewhere, context_pct, cache_pct, open) = view.get();
        (
            notice.map(|note| note.kind),
            spend.is_some(),
            elsewhere.map(|(_, waiting)| waiting),
            context_pct,
            cache_pct,
            open,
            place.with(|place| !place.is_empty()),
        )
    });
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .shrink(0.0)
        .min_width(0.0)
        .min_height(0.0)
        .children_reactive(move || {
            shape.with(|_| ());
            sigil::reactive::untrack(|| vec![footer_row(app.clone(), view, place, width)])
        })
}

const CONTEXT: &str = "ctx";
const CACHE: &str = "cache";
const BAR_CELLS: u16 = 8;

fn meter_cells(caption: &str, percent: Option<u64>) -> usize {
    percent.map_or(0, |percent| {
        cell_width(caption) + usize::from(BAR_CELLS) + cell_width(&format!("{percent}%")) + 4
    })
}

fn place_of(app: &App, view: &FooterView, width: usize) -> String {
    let (notice, spend, elsewhere, context_pct, cache_pct, _) = view;
    let taken = meter_cells(CONTEXT, *context_pct)
        + meter_cells(CACHE, *cache_pct)
        + elsewhere
            .as_ref()
            .map_or(0, |(note, _)| cell_width(note) + 2)
        + app
            .git_branch()
            .as_deref()
            .map_or(0, |branch| cell_width(branch) + 4)
        + spend.as_deref().map_or(0, |cost| cell_width(cost) + 2)
        + notice.as_ref().map_or(0, |note| cell_width(&note.text) + 2);
    crate::cells::ellipsize_start(&app.info.workspace, width.saturating_sub(taken))
}

fn footer_row(
    app: App,
    view: sigil::prelude::Signal<FooterView>,
    place: sigil::prelude::Signal<String>,
    width: usize,
) -> NodeBuilder {
    let (notice, spend, elsewhere, context_pct, cache_pct, open) = view.peek();
    let mut row = sigil::prelude::row()
        .fill_width()
        .align_items(AlignItems::Center)
        .gap(Space::Md);
    if place.with(|place| !place.is_empty()) {
        row = row.child(TextStyle::xs(Token::Gray12).line(place));
    }
    if let Some(branch) = &app.git_branch() {
        row = row.child(
            sigil::prelude::row()
                .align_items(AlignItems::Center)
                .shrink(0.0)
                .gap(Space::Sm)
                .child(TextStyle::xs(Token::Gray11).line("⎇"))
                .child(TextStyle::xs(Token::Warning11).line(branch.clone())),
        );
    }
    row = row.child(spacer());
    if let Some(note) = notice {
        let tone = match note.kind {
            super::super::app::NoticeKind::Copied => Token::Success11,
            super::super::app::NoticeKind::Failed => Token::Error11,
        };
        row = row.child(TextStyle::xs(tone).line(move || {
            view.with(|view| {
                view.0
                    .as_ref()
                    .map(|note| note.text.clone())
                    .unwrap_or_default()
            })
        }));
    }
    if let Some((_, waiting)) = elsewhere {
        let tone = if waiting {
            Token::Warning11
        } else {
            Token::Gray11
        };
        row = row.child(TextStyle::xs(tone).line(move || {
            view.with(|view| {
                view.2
                    .as_ref()
                    .map(|(note, _)| note.clone())
                    .unwrap_or_default()
            })
        }));
    }
    if spend.is_some() {
        row = row.child(
            TextStyle::xs(Token::Gray12)
                .line(move || view.with(|view| view.1.clone().unwrap_or_default())),
        );
    }
    let meters = [
        (
            false,
            Target::Context,
            CONTEXT,
            context_pct,
            sigil::prelude::Tone::Primary,
        ),
        (
            true,
            Target::Cache,
            CACHE,
            cache_pct,
            sigil::prelude::Tone::Info,
        ),
    ];
    for (cache, target, caption, percent, fill) in meters {
        let Some(percent) = percent else {
            continue;
        };
        let card = open
            .as_ref()
            .and_then(|(shown, rows)| (*shown == cache).then(|| rows.clone()));
        row = row.child(meter_button(
            app.clone(),
            target,
            meter(caption, percent, fill),
            card,
            width,
        ));
    }
    row
}

#[cfg(test)]
#[path = "footer_tests.rs"]
mod tests;
