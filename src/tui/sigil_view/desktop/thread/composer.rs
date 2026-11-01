use super::*;
use crate::tui::sigil_view::view::editor;
use sigil::prelude::Radius;

const PLACEHOLDER: &str = "Ask anything; @ attaches a file, / runs a command";
const EDITING: &str = "Editing the file: ctrl+s saves, esc leaves";
const ATTACH: &str = "Attach a file";
const EFFORT_OFF: &str = "effort off";
const APPROVAL_AUTO: &str = "auto";
const APPROVAL_ASK: &str = "ask on risky";

pub(super) fn composer(app: App, editing: bool) -> NodeBuilder {
    let typing = app.clone();
    let mut composer = container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_top(S)
                .shrink(0.0)
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
        .child(crate::tui::sigil_view::effort::strip(app.clone()))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .padding_x(M)
                        .padding_y(S)
                        .gap(S),
                )
                .rounded(Radius::Lg)
                .bg(Token::SurfaceSubtle)
                .border_w(1)
                .border_color(Token::Border)
                .child(mentions::chips(app.clone(), editing))
                .child(
                    editor(&app, editing)
                        .placeholder(if editing { EDITING } else { PLACEHOLDER })
                        .autofocus()
                        .layout(LayoutStyle::flex_column().fill_width().height(72.0))
                        .font_size(BODY)
                        .text_color(Token::OnSurface),
                )
                .child(tools(&app, editing)),
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

fn tools(app: &App, editing: bool) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .align_items(AlignItems::Center)
                .gap(XS),
        )
        .child(attach(app.clone()))
        .child(model_chip(app.clone()))
        .child(effort_chip(app.clone()))
        .child(approval_chip(app.clone()))
        .child(spacer())
        .child(send(app.clone(), editing))
}

fn chip(label: impl Fn() -> String + 'static, name: &'static str) -> sigil::widgets::ButtonBuilder {
    button(label)
        .ghost()
        .size(ControlSize::Compact)
        .accessible(name)
}

fn attach(app: App) -> NodeBuilder {
    icon_button("@", move || {
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

fn model_chip(app: App) -> NodeBuilder {
    let model = app.model;
    let name = sigil::reactive::derived(move || model.get().snapshot.model.clone());
    chip(move || name.get(), "Model")
        .activate(move || app.begin_model_edit())
        .into()
}

fn effort_chip(app: App) -> NodeBuilder {
    let model = app.model;
    let name = sigil::reactive::derived(move || {
        model.get().snapshot.effort.map_or_else(
            || EFFORT_OFF.to_string(),
            |level| format!("effort {}", level.as_str()),
        )
    });
    chip(move || name.get(), "Effort")
        .activate(move || {
            if app.effort.open.peek() {
                app.effort.open.set(false);
            } else {
                let snapshot = app.model.peek().snapshot.clone();
                app.effort.show(snapshot.effort, &snapshot.levels);
            }
        })
        .into()
}

fn approval_chip(app: App) -> NodeBuilder {
    let model = app.model;
    let name = sigil::reactive::derived(move || {
        if model.get().snapshot.approval {
            APPROVAL_ASK.to_string()
        } else {
            APPROVAL_AUTO.to_string()
        }
    });
    chip(move || name.get(), "Approval")
        .activate(move || drop(app.cmd.send(crate::tui::Cmd::ToggleApproval)))
        .into()
}

fn send(app: App, editing: bool) -> NodeBuilder {
    let gate = app.clone();
    let label = app.clone();
    let draft = app.draft;
    let attached = app.attached;
    button(move || {
        if running(&label) {
            "Stop".to_string()
        } else {
            "Send".to_string()
        }
    })
    .primary()
    .size(ControlSize::Compact)
    .disabled(move || {
        !running(&gate) && (editing || (draft.get().trim().is_empty() && attached.get() == 0))
    })
    .activate(move || {
        if running(&app) {
            app.interrupt();
        } else {
            app.submit();
        }
    })
    .into()
}

fn running(app: &App) -> bool {
    app.model
        .get()
        .busy_since
        .is_some_and(|(activity, _)| activity != crate::tui::Activity::Branch)
}

#[cfg(test)]
mod tests {
    use sigil::app::FrameHooks;

    use sigil::Host;
    use sigil::scene::SceneCommand;
    use tokio::sync::mpsc::unbounded_channel;

    use super::super::App;
    use crate::tui::{Fact, Msg};

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
                crate::tui::sigil_view::profile::info(),
                workspace.path().to_owned(),
                commands,
            );
            if busy {
                app.update(Msg::Fact(Fact::Busy(Some(crate::tui::Activity::Turn))));
            }
            app.refresh();
            let mut host = Host::new();
            let tree = app.clone();
            host.mount(move || super::composer(tree, false));
            host.frame((1200.0, 800.0), 1.0, FrameHooks::default());
            host.scene((1200.0, 800.0))
                .expect("scene")
                .commands
                .into_iter()
                .filter_map(|command| match command {
                    SceneCommand::GlyphRun(draw) => Some(draw.run.text.clone()),
                    _ => None,
                })
                .collect()
        })
    }
}
