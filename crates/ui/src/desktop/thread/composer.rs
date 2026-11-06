use super::*;
use crate::view::surface::editor;
use sigil::prelude::Widget;
use sigil::prelude::{Dimension, Radius};

const PLACEHOLDER: &str = "Message";
const FIRST: &str = "What should this agent do?";
const EDITING: &str = "Editing";
const ATTACH: &str = "Attach a file";

pub(super) fn composer(app: App, editing: bool) -> NodeBuilder {
    let typing = app.clone();
    let (menu, shown) = (sigil::prelude::signal(false), app.clone());
    let mut composer = container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_top(S)
                .shrink(0.0)
                .position(sigil::prelude::Position::Relative)
                .align_items(AlignItems::Stretch),
        )
        .pointer_down(move |_| {
            if typing.pane_focus.peek() {
                typing.focus_pane(false);
            }
            typing.focus_composer();
        })
        .child(chat::suggest::widget(
            app.clone(),
            (COLUMN / chat::canvas::cell().w) as usize,
        ))
        .child(crate::effort::strip(app.clone()))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .position(sigil::prelude::Position::Absolute)
                        .inset_bottom(Dimension::Percent(1.0))
                        .inset_right(0.0)
                        .padding_bottom(XS)
                        .align_items(AlignItems::End),
                )
                .children_reactive(move || super::model_menu::panel(&shown, menu)),
        )
        .child(
            container()
                .layout(LayoutStyle::flex_column().fill_width().padding(S))
                .rounded(Radius::Lg)
                .shadow(super::super::style::FLOATING)
                .bg(super::super::style::RAISED)
                .border_w(1)
                .border_color(super::super::style::OUTLINE)
                .child({
                    let queued = app.clone();
                    container()
                        .layout(LayoutStyle::block().fill_width())
                        .children_reactive(move || {
                            super::queue::panel(&queued)
                                .into_iter()
                                .collect::<Vec<NodeBuilder>>()
                        })
                })
                .child(mentions::chips(app.clone(), editing))
                .child(row(&app, editing, menu)),
        );
    for modifiers in [ModifiersState::CONTROL, ModifiersState::META] {
        let paste = app.clone();
        composer = composer.key_capture(
            Shortcut::new(LogicalKey::character("v"), modifiers),
            move |event| {
                if paste.clipboard() {
                    event.stop_propagation();
                }
            },
        );
    }
    composer
}

fn row(app: &App, editing: bool, menu: sigil::prelude::Signal<bool>) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_column().fill_width().gap(S))
        .child(
            editor(app, editing)
                .placeholder({
                    let model = app.model;
                    move || {
                        let shown = if editing {
                            EDITING
                        } else if model.get().entries.asked() {
                            PLACEHOLDER
                        } else {
                            FIRST
                        };
                        shown.to_string()
                    }
                })
                .autofocus()
                .chrome(false)
                .focus_ring(sigil::prelude::signal(false))
                .content_padding(
                    (super::super::style::CONTROL - super::super::style::MARK) / 2.0
                        - super::super::style::CARET,
                    2.0,
                )
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .min_width(0.0)
                        .height(Dimension::Auto)
                        .min_size(0.0, LINE)
                        .max_size(Dimension::Auto, 192.0)
                        .shrink(0.0),
                )
                .line_height_px(LINE)
                .font_size(BODY)
                .text_color(Token::OnSurface),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .gap(XS)
                        .padding_right(
                            (super::super::style::CONTROL - super::super::style::MARK) / 2.0,
                        )
                        .align_items(AlignItems::Center),
                )
                .child(attach(app.clone()))
                .child(super::place::chip(app))
                .child(approval(app.clone()))
                .child(spacer())
                .child({
                    let ring = app.clone();
                    container()
                        .layout(
                            LayoutStyle::flex_row()
                                .shrink(0.0)
                                .align_items(AlignItems::Center),
                        )
                        .children_reactive(move || {
                            super::status::ring(&ring)
                                .into_iter()
                                .collect::<Vec<NodeBuilder>>()
                        })
                })
                .child(super::model_menu::chip(app, menu))
                .child(send(app.clone(), editing)),
        )
}

/// The chip that shows and toggles whether risky steps wait for the user's OK.
fn approval(app: App) -> NodeBuilder {
    let shown = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .shrink(0.0)
                .align_items(AlignItems::Center),
        )
        .children_reactive(move || {
            let snapshot = shown.model.get().snapshot.clone();
            let (label, locked) = if snapshot.strict {
                ("Strict", true)
            } else if snapshot.approval {
                ("Ask first", false)
            } else {
                ("Allow all", false)
            };
            let toggle = shown.clone();
            vec![NodeBuilder::from(
                button(label)
                    .ghost()
                    .size(ControlSize::Compact)
                    .disabled(sigil::prelude::signal(locked))
                    .activate(move || {
                        drop(toggle.cmd.send(summonr_agent::hub::Cmd::ToggleApproval))
                    }),
            )]
        })
}

fn attach(app: App) -> NodeBuilder {
    icon_button(sigil::icons::Icon::Plus, move || {
        if app.view_edit.peek() {
            return;
        }
        let draft = app.draft.peek();
        let gap = if draft.is_empty() || draft.ends_with(char::is_whitespace) {
            ""
        } else {
            " "
        };
        app.edit(&format!("{draft}{gap}@"));
        app.focus_composer();
    })
    .accessible(ATTACH)
    .into()
}

fn send(app: App, editing: bool) -> NodeBuilder {
    let shown = app.clone();
    container().children_reactive(move || -> Vec<NodeBuilder> {
        let app = shown.clone();
        if running(&app) {
            return vec![
                icon_button(sigil::icons::Icon::Square, move || app.interrupt())
                    .primary()
                    .solid()
                    .accessible("Stop")
                    .rounded(Radius::Full)
                    .into(),
            ];
        }
        let (draft, attached, submit) = (app.draft, app.attached, app.clone());
        vec![
            icon_button(sigil::icons::Icon::ArrowUp, move || submit.submit())
                .primary()
                .solid()
                .accessible("Send")
                .disabled(move || editing || (draft.get().trim().is_empty() && attached.get() == 0))
                .rounded(Radius::Full)
                .into(),
        ]
    })
}

fn running(app: &App) -> bool {
    app.model
        .get()
        .busy_since
        .is_some_and(|(activity, _)| activity != summonr_agent::hub::Activity::Branch)
}

#[cfg(test)]
mod tests {
    use sigil::app::FrameHooks;

    use sigil::Host;
    use tokio::sync::mpsc::unbounded_channel;

    use super::super::App;
    use summonr_agent::hub::{Fact, Msg};

    #[test]
    fn stop_replaces_send_while_a_turn_is_running() {
        let idle = labels(false);
        let running = labels(true);
        assert!(
            idle.iter().any(|text| text == "Send") && !idle.iter().any(|text| text == "Stop"),
            "idle composer: {idle:?}"
        );
        assert!(
            running.iter().any(|text| text == "Stop") && !running.iter().any(|text| text == "Send"),
            "running composer: {running:?}"
        );
    }

    fn labels(busy: bool) -> Vec<String> {
        sigil::reactive::Owner::scope("summonr-stop", || {
            let workspace = tempfile::tempdir().expect("workspace");
            let (commands, _receiver) = unbounded_channel();
            let app = App::new(
                crate::profile::info(),
                workspace.path().to_owned(),
                commands,
            );
            if busy {
                app.update(Msg::Fact(Fact::Busy(Some(
                    summonr_agent::hub::Activity::Turn,
                ))));
            }
            app.refresh();
            let mut host = Host::new(sigil::prelude::HostMode::Pixel);
            let tree = app.clone();
            host.mount(move || super::composer(tree, false));
            host.frame((1200.0, 800.0), 1.0, FrameHooks::default());
            host.accessibility_tree()
                .nodes
                .iter()
                .filter_map(|(_, node)| node.label().map(str::to_string))
                .collect()
        })
    }
}
