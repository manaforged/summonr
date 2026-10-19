use std::rc::Rc;

use sigil::kernel::TextContent;
use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, ListRowSlots, NodeBuilder, Tone, WidgetLayoutExt,
    button, container, icon_button, list_row_with, section_label, sidebar, signal, spacer, tooltip,
};

use super::{App, Pane};
use super::{icons, open};

mod session_rows;
#[cfg(test)]
pub(super) use session_rows::recent;

const RAIL: f32 = 252.0;
const COMPACT_RAIL: f32 = 56.0;
const NARROW: f32 = 900.0;
const SECTION_GAP: f32 = 12.0;
pub(super) const NEW_SESSION: &str = "New session";
pub(super) const NEW_WORKTREE: &str = "New session in a worktree";
pub(super) const UNTITLED: &str = "Untitled";
pub(super) const LEDGER: &str = "Ledger";
const SESSIONS: &str = "Sessions";
const ADD_PROVIDER: &str = "Add a provider";
const PANES: [(Pane, &str); 6] = [
    (Pane::Files, "Files"),
    (Pane::Diff, "Changes"),
    (Pane::Worktrees, "Worktrees"),
    (Pane::Jobs, "Jobs"),
    (Pane::Peers, "Peers"),
    (Pane::Ledger, LEDGER),
];

pub(super) fn rail(app: App) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_column().fill_height().shrink(0.0))
        .children_reactive(move || {
            let narrow = sigil::app::viewport_size().get().width < NARROW;
            vec![body(&app, narrow)]
        })
}

fn body(app: &App, narrow: bool) -> NodeBuilder {
    let mut column = container().layout(
        LayoutStyle::flex_column()
            .fill_width()
            .fill_height()
            .padding_x(12.0)
            .padding_y(14.0)
            .gap(2.0),
    );
    column = column.child(panes(app, narrow));
    column = if narrow {
        column.child(new_session(app))
    } else {
        column
            .child(heading(app))
            .child(session_rows::sessions(app))
    };
    container()
        .layout(LayoutStyle::block().fill_height())
        .child(
            sidebar(column.child(spacer()).child(seat(app, narrow))).width(if narrow {
                COMPACT_RAIL
            } else {
                RAIL
            }),
        )
}

fn panes(app: &App, narrow: bool) -> NodeBuilder {
    let flow = if narrow {
        LayoutStyle::flex_column()
    } else {
        LayoutStyle::flex_row()
    };
    let mut strip = container()
        .label("Panes")
        .layout(flow.fill_width().gap(2.0).align_items(AlignItems::Center));
    for (pane, title) in PANES {
        let toggle = app.clone();
        let shown = open(app, pane);
        let control =
            icon_button(icons::pane(pane), move || toggle.toggle_pane(pane)).accessible(title);
        let control = if shown { control.soft() } else { control };
        strip = strip.child(
            container()
                .selected(signal(shown))
                .child(tooltip(title).child(control)),
        );
    }
    strip
}

fn heading(app: &App) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_top(SECTION_GAP)
                .align_items(AlignItems::Center),
        )
        .child(
            container()
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0))
                .child(section_label(SESSIONS)),
        )
        .child(new_worktree(app))
        .child(new_session(app))
}

fn new_session(app: &App) -> NodeBuilder {
    let open = app.clone();
    container().child(
        tooltip(NEW_SESSION)
            .child(icon_button(icons::NEW, move || open.new_session()).accessible(NEW_SESSION)),
    )
}

fn new_worktree(app: &App) -> NodeBuilder {
    let open = app.clone();
    container().child(tooltip(NEW_WORKTREE).child(
        icon_button(icons::WORKTREE, move || open.new_worktree_session()).accessible(NEW_WORKTREE),
    ))
}

fn section(title: impl Into<TextContent>) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_top(SECTION_GAP),
        )
        .child(section_label(title))
}

fn seat(app: &App, narrow: bool) -> NodeBuilder {
    let open = app.clone();
    let unseated = app.model.get().snapshot.unseated;
    if narrow {
        let label = if unseated {
            ADD_PROVIDER.to_string()
        } else {
            app.info.endpoint.clone()
        };
        let control =
            icon_button(icons::SEAT, move || open.open_providers()).accessible(label.clone());
        let control = if unseated {
            control.tone(Tone::Warn)
        } else {
            control
        };
        return container().child(tooltip(label).child(control));
    }
    if unseated {
        return container().child(
            button(ADD_PROVIDER)
                .tone(Tone::Warn)
                .soft()
                .activate(move || open.open_providers()),
        );
    }
    let model = app.model.get().snapshot.model.clone();
    list_row_with(
        false,
        app.info.endpoint.clone(),
        ListRowSlots {
            status: Some(model.into()),
            activate: Some(Rc::new(move || open.open_providers())),
            ..ListRowSlots::default()
        },
    )
}
