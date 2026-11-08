use sigil::prelude::Position;
use sigil::prelude::Widget;
use sigil::prelude::{LayoutStyle, NodeBuilder, Paint, container};

use crate::view::keys::shell;
use summonr_agent::hub::Cmd;
use summonr_agent::hub::Status;

pub(super) use super::app::App;
pub(super) use super::chat;
pub(super) use super::pane::Pane;

mod agent_menu;
mod attention;
mod brand;
pub(super) mod entry;
mod friends;
pub(crate) mod host;
pub(crate) mod launch;
#[cfg(feature = "menu")]
mod menubar;
pub(super) mod palette;
mod panel;
pub(super) mod quit;
mod restore;
mod settings;
mod shelf;
mod ship;
mod side;
mod side_panel;
mod sidebar;
mod sky;
mod spaces;
pub(crate) mod split;
pub(crate) mod style;
pub(crate) mod terminal;
mod thread;
mod topic;
pub(super) mod welcome;
mod will;

/// The height of the bar the window controls sit in.
pub(super) const TITLE_BAR: f32 = sidebar::BAR;
/// Room left of the bar's first control for the macOS window controls.
const CONTROLS: f32 = 84.0;

/// The button that shows or hides the sidebar.
fn sidebar_toggle(app: &App) -> NodeBuilder {
    let side = app.clone();
    let toggle = sigil::prelude::icon_button(sigil::icons::Icon::PanelLeft, move || {
        side.window.sidebar.set(!side.window.sidebar.peek());
    })
    .accessible(SIDEBAR);
    let watch = app.clone();
    let count = container()
        .layout(LayoutStyle::flex_row().shrink(0.0))
        .children_reactive(move || {
            let needing = watch
                .model
                .get()
                .live
                .iter()
                .filter(|entry| entry.status == Status::NeedsYou)
                .count();
            if needing == 0 {
                return Vec::new();
            }
            vec![NodeBuilder::from(
                style::text(style::Text::Meta, needing.to_string()).text_color(style::NEEDS_DOT),
            )]
        });
    container()
        .layout(
            LayoutStyle::flex_row()
                .shrink(0.0)
                .align_items(sigil::prelude::AlignItems::Center),
        )
        .child(tip(SIDEBAR, Some("⌘B"), toggle.into()))
        .child(count)
        .into()
}

/// Wraps `node` in a hover tooltip naming it and its shortcut.
fn tip(label: &str, key: Option<&str>, node: NodeBuilder) -> NodeBuilder {
    sigil::widgets::tooltip(label).hint(key).child(node).into()
}

const SIDEBAR: &str = "Sidebar";

/// Whether an agent dialog holds the keyboard.
pub(crate) fn dialog_open() -> bool {
    agent_menu::is_open() || settings::is_open()
}

/// Opens the agent a window shortcut names.
pub(crate) fn jump(app: &App, to: crate::view::keys::bindings::Jump) {
    sidebar::jump(app, to);
}

/// Empty bar space that moves the window when dragged.
fn drag_area() -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::block()
                .width(0.0)
                .grow(1.0)
                .fill_height()
                .min_size(0.0, 0.0),
        )
        .pointer_down(|_| {
            sigil::events::request_window_command(sigil::app::WindowCommand::Drag);
        })
}

pub(super) fn root(app: App) -> NodeBuilder {
    drop(app.cmd.send(Cmd::Diff(None)));
    sigil::style::text_scale(style::TEXT_SCALE);
    palette::install();
    agent_menu::install();
    settings::install(&app);
    split::install(&app);
    panel::install(&app);
    shelf::install();
    spaces::install(&app);
    side_panel::install(&app);
    restore::install(&app);
    quit::install(&app);
    #[cfg(feature = "menu")]
    menubar::install(&app);
    attention::announce(&app);
    let paint = app.clone();
    shell(&app).children_reactive(move || {
        vec![window(paint.clone()).theme(crate::desktop::style::gui(paint.appearance.get()))]
    })
}

/// Looks for providers installed on this machine and offers them while none is set up.
pub(super) fn probe_local(app: &App) {
    friends::refresh(app);
    let local = app.window.local;
    let (out, inbox) = tokio::sync::mpsc::unbounded_channel();
    sigil::tasks::on_message(inbox, move |found| local.set(found), || {});
    tokio::task::spawn_blocking(move || drop(out.send(summonr_agent::endpoint::found_locally())));
}

fn window(app: App) -> NodeBuilder {
    let overlays = app.clone();
    let menu = app.clone();
    let dialogs = app.clone();
    container()
        .layout(LayoutStyle::flex_column().fill())
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill()
                        .grow(1.0)
                        .min_size(0.0, 0.0)
                        .position(Position::Relative),
                )
                .child(content(app.clone()))
                .children_reactive(move || {
                    app.settings_open.get().then(|| {
                        container()
                            .layout(
                                LayoutStyle::flex_column()
                                    .position(Position::Absolute)
                                    .fill(),
                            )
                            .child(sigil::reactive::untrack(|| settings::view(&app)))
                    })
                }),
        )
        .child(slot().children_reactive(move || palette::view(&menu)))
        .child(slot().children_reactive(move || agent_menu::view(&dialogs)))
        .children_reactive(move || {
            let _viewport = overlays.viewport.get();
            let main = overlays
                .overlay_beyond(super::app::Overlay::Settings)
                .map(|overlay| (overlay, overlays.clone()));
            let shown = main.or_else(|| {
                let pane = split::current(&overlays);
                pane.overlay_beyond(super::app::Overlay::Settings)
                    .map(|overlay| (overlay, pane))
            });
            shown
                .and_then(|(overlay, owner)| overlay.layer(owner))
                .map(|layer| {
                    container()
                        .layout(
                            LayoutStyle::flex_column()
                                .position(Position::Absolute)
                                .fill(),
                        )
                        .child(layer)
                })
        })
}

/// An empty slot for one popup; the popup draws above the window.
fn slot() -> NodeBuilder {
    container().layout(
        LayoutStyle::block()
            .position(Position::Absolute)
            .width(0.0)
            .height(0.0),
    )
}

fn content(app: App) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_row().fill().min_size(0.0, 0.0))
        .child(
            container()
                .layout(LayoutStyle::flex_column().fill_height().shrink(0.0))
                .children_reactive({
                    let app = app.clone();
                    move || -> Vec<NodeBuilder> {
                        if app.window.sidebar.get()
                            && sigil::app::viewport_size().get().width >= sidebar::ROOMY
                        {
                            vec![sigil::reactive::untrack(|| sidebar::view(app.clone()))]
                        } else {
                            Vec::new()
                        }
                    }
                }),
        )
        .child(sigil::reactive::untrack(split::view))
        .child(
            container()
                .layout(LayoutStyle::flex_column().fill_height().shrink(0.0))
                .children_reactive(move || {
                    let focused = split::current(&app);
                    let layout = focused.layout.get();
                    let (_, side_cols) = layout.split(focused.viewport.get().0);
                    match layout.secondary {
                        Some(pane) if side_cols > 0 => {
                            vec![side::column(&focused, pane, side_cols)]
                        }
                        Some(_) | None => Vec::new(),
                    }
                }),
        )
}

#[cfg(test)]
mod tests;
