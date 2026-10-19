use super::*;
use sigil::prelude::Radius;

const PLACEHOLDER: &str = "Ask anything; @ attaches a file, / runs a command";
const EDITING: &str = "Editing the file: ctrl+s saves, esc leaves";
const ATTACH: &str = "Attach a file";
const EFFORT_OFF: &str = "effort off";
const APPROVAL_AUTO: &str = "auto";
const APPROVAL_ASK: &str = "ask on risky";

pub(super) fn composer(app: App, editing: bool) -> NodeBuilder {
    let changed = app.clone();
    let paste = app.clone();
    let paste_meta = app.clone();
    let (value, caret) = if editing {
        (app.view_text, app.view_caret)
    } else {
        (app.draft, app.caret)
    };
    let typing = app.clone();
    container()
        .key_capture(Shortcut::ctrl(LogicalKey::character("v")), move |event| {
            if paste.clipboard() {
                event.stop_propagation();
            }
        })
        .key_capture(
            Shortcut::new(LogicalKey::character("v"), ModifiersState::META),
            move |event| {
                if paste_meta.clipboard() {
                    event.stop_propagation();
                }
            },
        )
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
                    text_edit(value)
                        .caret_position(caret)
                        .placeholder(if editing { EDITING } else { PLACEHOLDER })
                        .change(move |draft| {
                            if !editing {
                                changed.change(draft);
                            }
                        })
                        .focus_when(app.editor_focus)
                        .autofocus()
                        .layout(LayoutStyle::flex_column().fill_width().height(72.0))
                        .font_size(BODY)
                        .text_color(Token::OnSurface),
                )
                .child(tools(&app, editing)),
        )
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
    let draft = app.draft;
    let attached = app.attached;
    button("Send")
        .primary()
        .size(ControlSize::Compact)
        .disabled(move || editing || (draft.get().trim().is_empty() && attached.get() == 0))
        .activate(move || app.submit())
        .into()
}
