use sigil::prelude::Position;
use sigil::prelude::Widget;
use sigil::prelude::{LayoutStyle, NodeBuilder, container};

use super::view::shell;
use crate::tui::Cmd;
use crate::tui::hub::Status;

pub(super) use super::app::App;
pub(super) use super::chat;
pub(super) use super::pane::Pane;

mod brand;
pub(super) mod entry;
mod agent_menu;
pub(in crate::tui::sigil_view) mod split;
pub(super) mod palette;
mod sky;
mod settings;
mod sidebar;
mod side;
mod style;
pub(in crate::tui::sigil_view) mod terminal;
mod thread;
pub(super) mod welcome;

/// The height of the bar the window controls sit in.
pub(super) const TITLE_BAR: f32 = sidebar::BAR;
/// Room left of the bar's first control for the macOS window controls.
const CONTROLS: f32 = 84.0;

/// The button that shows or hides the sidebar.
fn sidebar_toggle(app: &App) -> NodeBuilder {
    let side = app.clone();
    let toggle = sigil::prelude::icon_button(sigil::icons::Icon::PanelLeft, move || {
        side.sidebar.set(!side.sidebar.peek());
    })
    .accessible(SIDEBAR);
    tip(SIDEBAR, Some("⌘B"), toggle.into())
}

/// Wraps `node` in a hover tooltip naming it and its shortcut.
fn tip(label: &str, key: Option<&str>, node: NodeBuilder) -> NodeBuilder {
    sigil::widgets::tooltip(label).hint(key).child(node).into()
}

const SIDEBAR: &str = "Sidebar";

/// Sends a desktop notification when an agent out of view starts to need the user or finishes.
fn announce(app: &App) {
    let watch = app.clone();
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, Status)>::new()));
    sigil::prelude::effect(move || {
        let model = watch.model.get();
        let mut before = seen.borrow_mut();
        for entry in &model.live {
            let was = before.iter().find(|(id, _)| *id == entry.session).map(|(_, status)| *status);
            let word = match entry.status {
                Status::NeedsYou => NEEDS_YOU,
                Status::Review => FINISHED,
                _ => continue,
            };
            if was.is_some_and(|was| was != entry.status) && !split::shows(&entry.session) {
                drop(sigil::app::notify::Notification::new(entry.title.clone(), word).send());
            }
        }
        *before = model.live.iter().map(|entry| (entry.session.clone(), entry.status)).collect();
    });
}

const NEEDS_YOU: &str = "Needs you";
const FINISHED: &str = "Finished";

/// Whether an agent dialog holds the keyboard.
pub(in crate::tui::sigil_view) fn dialog_open() -> bool {
    agent_menu::is_open() || settings::is_open()
}

/// Opens the agent a window shortcut names.
pub(in crate::tui::sigil_view) fn jump(app: &App, to: super::view::Jump) {
    sidebar::jump(app, to);
}

/// Empty bar space that moves the window when dragged.
fn drag_area() -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().width(0.0).grow(1.0).fill_height().min_size(0.0, 0.0))
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
    announce(&app);
    let paint = app.clone();
    shell(&app).children_reactive(move || {
        vec![window(paint.clone()).theme(super::theme::gui(paint.appearance.get()))]
    })
}

/// Looks for providers installed on this machine and offers them while none is set up.
pub(super) fn probe_local(app: &App) {
    let local = app.local;
    let (out, inbox) = tokio::sync::mpsc::unbounded_channel();
    sigil::tasks::on_message(inbox, move |found| local.set(found), || {});
    tokio::task::spawn_blocking(move || drop(out.send(crate::endpoint::found_locally())));
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
                            .layout(LayoutStyle::flex_column().position(Position::Absolute).fill())
                            .child(sigil::reactive::untrack(|| settings::view(&app)))
                    })
                }),
        )
        .child(slot().children_reactive(move || palette::view(&menu)))
        .child(slot().children_reactive(move || agent_menu::view(&dialogs)))
        .children_reactive(move || {
            let _viewport = overlays.viewport.get();
            let main = overlays.overlay_beyond(super::app::Overlay::Settings).map(|overlay| (overlay, overlays.clone()));
            let shown = main.or_else(|| {
                let pane = split::current(&overlays);
                pane.overlay_beyond(super::app::Overlay::Settings).map(|overlay| (overlay, pane))
            });
            shown
                .and_then(|(overlay, owner)| overlay.layer(owner))
                .map(|layer| {
                    container()
                        .layout(LayoutStyle::flex_column().position(Position::Absolute).fill())
                        .child(layer)
                })
        })
}

/// An empty slot for one popup; the popup draws above the window.
fn slot() -> NodeBuilder {
    container().layout(LayoutStyle::block().position(Position::Absolute).width(0.0).height(0.0))
}

fn content(app: App) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_row().fill().min_size(0.0, 0.0))
        .child(container().layout(LayoutStyle::flex_column().fill_height().shrink(0.0)).children_reactive({
            let app = app.clone();
            move || -> Vec<NodeBuilder> {
                if app.sidebar.get() && sigil::app::viewport_size().get().width >= sidebar::ROOMY {
                    vec![sigil::reactive::untrack(|| sidebar::view(app.clone()))]
                } else {
                    Vec::new()
                }
            }
        }))
        .child(sigil::reactive::untrack(split::view))
        .child(container().layout(LayoutStyle::flex_column().fill_height().shrink(0.0)).children_reactive(move || {
            let focused = split::current(&app);
            let layout = focused.layout.get();
            let (_, side_cols) = layout.split(focused.viewport.get().0);
            match layout.secondary {
                Some(pane) if side_cols > 0 => vec![side::column(&focused, pane, side_cols)],
                Some(_) | None => Vec::new(),
            }
        }))
}

#[cfg(test)]
mod tests;
