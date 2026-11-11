use sigil::prelude::{AlignItems, NodeBuilder, Widget, container};

use super::super::App;
use super::super::style::{M, S, Text, XS};

/// The strip over the chat: the toggle and agent name while the sidebar is hidden, then the diff stat and Terminal.
/// The title strip of a pane.
pub(in crate::desktop) fn header(app: &App) -> NodeBuilder {
    let (watch, items) = (app.clone(), app.clone());
    let beside = sigil::prelude::derived(move || {
        watch.window.sidebar.get()
            && sigil::app::viewport_size().get().width >= super::super::sidebar::ROOMY
    });
    sigil::prelude::row()
        .fill_width()
        .height(super::super::TITLE_BAR)
        .shrink(0.0)
        .padding_left(S)
        .padding_right(M)
        .gap(XS)
        .align_items(AlignItems::Center)
        .children_reactive(move || header_items(&items, beside.get()))
}

fn header_items(app: &App, beside: bool) -> Vec<NodeBuilder> {
    let split = super::super::split::active();
    let corner = !beside && super::super::split::is_first(app);
    let mut items: Vec<NodeBuilder> = Vec::new();
    if corner {
        items.push(
            container()
                .width(super::super::CONTROLS - 8.0)
                .shrink(0.0)
                .into(),
        );
        items.push(super::super::sidebar_toggle(app));
    }
    let terminal = app.shown_terminal();
    if split || !beside || terminal.is_some() {
        items.push(name(app));
    }
    if terminal.is_none() {
        items.push(super::super::topic::strip(app));
    }
    items.push(super::super::drag_area());
    items.extend(super::super::side::toggles(app));
    if let Some(term) = terminal {
        let (close, id) = (app.clone(), term.id);
        let button = sigil::prelude::icon_button(sigil::icons::Icon::X, move || {
            super::super::terminal::close(&close, id)
        })
        .accessible(CLOSE_TERMINAL);
        items.push(super::super::tip(CLOSE_TERMINAL, None, button.into()));
    }
    items.extend(run(app));
    let pane = app.clone();
    let button = sigil::prelude::icon_button(sigil::icons::Icon::Columns2, move || {
        super::super::split::split_app(&pane)
    })
    .accessible(SPLIT_PANE);
    items.push(super::super::tip(SPLIT_PANE, Some("⌘D"), button.into()));
    if split {
        let close = app.clone();
        let button = sigil::prelude::icon_button(sigil::icons::Icon::X, move || {
            super::super::split::close_app(&close)
        })
        .accessible(CLOSE_PANE);
        items.push(super::super::tip(CLOSE_PANE, Some("⌘W"), button.into()));
    }
    items
}

/// The Run button when the folder has a run action; a right-click lists every action.
fn run(app: &App) -> Option<NodeBuilder> {
    let rows = super::super::terminal::run_rows(&app.workspace());
    let first = rows.iter().find(|row| !row.cloud_only)?;
    let (start, id) = (app.clone(), first.id.clone());
    let button = sigil::prelude::icon_button(sigil::icons::Icon::Play, move || {
        super::super::terminal::run_action(&start, id.as_deref())
    })
    .accessible(RUN_BUTTON);
    let list = app.clone();
    let menu = sigil::widgets::context_menu(button, move || {
        super::super::terminal::run_rows(&list.workspace())
            .into_iter()
            .map(|row| {
                let (pick, id) = (list.clone(), row.id.clone());
                let item = sigil::widgets::MenuItem::new(row.label).disabled(row.cloud_only);
                match row.cloud_only {
                    true => item.hint(super::super::palette::CLOUD_ONLY),
                    false => item
                        .activate(move || super::super::terminal::run_action(&pick, id.as_deref())),
                }
            })
            .collect()
    });
    Some(super::super::tip(&first.label, None, menu))
}

const RUN_BUTTON: &str = "Run in terminal";
const CLOSE_PANE: &str = "Close pane";
const SPLIT_PANE: &str = "Split pane";
const CLOSE_TERMINAL: &str = "Close terminal";

/// The agent's title and repo, shown in the strip while the sidebar is hidden.
fn name(app: &App) -> NodeBuilder {
    let watch = app.clone();
    sigil::prelude::row()
        .gap(S)
        .padding_left(XS)
        .shrink(1.0)
        .min_width(0.0)
        .min_height(0.0)
        .align_items(AlignItems::Center)
        .children_reactive(move || {
            if let Some(term) = watch.shown_terminal() {
                return vec![
                    super::super::style::text(Text::Label, term.label())
                        .ellipsize()
                        .into(),
                    super::super::style::text(
                        Text::Meta,
                        super::super::sidebar::folder_name(&term.folder),
                    )
                    .nowrap()
                    .into(),
                ];
            }
            let model = watch.model.get();
            let session = model.session.clone();
            let live = model
                .live
                .iter()
                .find(|live| live.session == session && !live.title.is_empty())
                .map(|live| live.title.clone());
            let title = live.or_else(|| {
                watch
                    .sessions
                    .all_chats()
                    .into_iter()
                    .find(|info| info.id == session)
                    .map(|info| super::super::sidebar::title(&info))
            });
            title
                .into_iter()
                .map(|title| {
                    super::super::style::text(Text::Label, title)
                        .ellipsize()
                        .into()
                })
                .chain([super::super::style::text(
                    Text::Meta,
                    super::super::sidebar::project_name(&watch),
                )
                .nowrap()
                .into()])
                .collect::<Vec<NodeBuilder>>()
        })
}
