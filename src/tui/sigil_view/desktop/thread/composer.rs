use super::*;
use sigil::prelude::{Radius, signal};

const PLACEHOLDER: &str = "Ask anything; @ attaches a file, / runs a command";
const EDITING: &str = "Editing the file: ctrl+s saves, esc leaves";

pub(super) fn composer(app: App, editing: bool) -> NodeBuilder {
    let submit = app.clone();
    let changed = app.clone();
    let paste = app.clone();
    let paste_meta = app.clone();
    let (value, caret) = if editing {
        (app.view_text, app.view_caret)
    } else {
        (app.draft, app.caret)
    };
    let focus = signal(0_u64);
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
        .pointer_down(move |_| focus.update(|n| *n += 1))
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
                .child(
                    text_edit(value)
                        .caret_position(caret)
                        .placeholder(if editing { EDITING } else { PLACEHOLDER })
                        .change(move |draft| {
                            if !editing {
                                changed.change(draft);
                            }
                        })
                        .submit(move |_| submit.submit())
                        .focus_when(focus)
                        .layout(LayoutStyle::flex_column().fill_width().height(72.0))
                        .font_size(BODY)
                        .text_color(Token::OnSurface),
                )
                .child(tools(&app, editing)),
        )
}

fn tools(app: &App, editing: bool) -> NodeBuilder {
    let attach = app.clone();
    let mention = app.clone();
    let picker = app.clone();
    let send = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .align_items(AlignItems::Center)
                .gap(XS),
        )
        .child(
            icon_button("+", move || {
                attach.clipboard();
            })
            .accessible("Attach from clipboard"),
        )
        .child(
            icon_button("@", move || {
                if !mention.view_edit.peek() {
                    let draft = mention.draft.peek();
                    mention.edit(&format!("{draft}@"));
                }
            })
            .accessible("Attach a file"),
        )
        .child(
            button(model_label(app))
                .ghost()
                .size(ControlSize::Compact)
                .accessible("Model")
                .activate(move || picker.begin_model_edit()),
        )
        .child(spacer())
        .child(
            button("Send")
                .primary()
                .size(ControlSize::Compact)
                .disabled(editing)
                .activate(move || send.submit()),
        )
}
