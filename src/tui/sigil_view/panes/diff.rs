use sigil::prelude::{
    AlignItems, Cx, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt,
    NodeStyleExt, Tone, WidgetLayoutExt, badge, button, classes, container, label, list_row, node,
    request_focus, spacer, text_edit,
};

use super::App;
use super::body::header;

pub(super) fn diff(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.get();
    if let Some(path) = app.diff_view.peek() {
        let back = app.clone();
        column = column
            .child(header(
                &app,
                path.clone(),
                format!("vs {}", model.diff.base),
            ))
            .child(button("← all changes").ghost().activate(move || {
                back.pane_back();
            }));
        let Some((_, result)) = model.patch.as_ref().filter(|(seen, _)| *seen == path) else {
            return column.child(label("loading…").class(classes!("text-xs text-gray-11")));
        };
        let text = match result {
            Ok(text) => text,
            Err(error) => {
                return column.child(label(error.clone()).class(classes!("text-xs text-error-11")));
            }
        };
        let hunks = app.hunks();
        if hunks.is_empty() {
            return column.child(crate::tui::sigil_view::chat::entry::patch_view(
                &app,
                text.clone(),
                model.prefs.diff_layout,
            ));
        }
        let cursor = app.hunk_cursor.peek();
        let notes = format!(
            "{} hunks · enter accepts · r reverts · n notes",
            hunks.len()
        );
        column = column.child(label(notes).class(classes!("text-xs text-gray-11")));
        drop(model);
        for (index, hunk) in hunks.iter().enumerate() {
            column = column.child(hunk_view(&app, &path, index, hunk, index == cursor));
        }
        return column;
    }
    column = column.child(header(&app, "Changes".to_string(), model.diff.summary()));
    drop(model);
    let stats = app.diff_stats();
    if stats.is_empty() {
        return column.child(label("no changes").class(classes!("text-xs text-gray-11")));
    }
    for (index, stat) in stats.into_iter().enumerate() {
        let open = app.clone();
        let path = stat.path.clone();
        let menu = app.clone();
        column = column.child(
            list_row(
                index == app.diff_cursor.peek(),
                stat.path.clone(),
                format!("+{} −{}", stat.added, stat.removed),
            )
            .pointer_down(move |event| {
                if event.button == Some(MouseButton::Right) {
                    menu.context_row(index);
                } else {
                    open.diff_cursor.set(index);
                    open.diff_open(path.clone());
                }
                event.stop_propagation();
            }),
        );
    }
    column
}

fn hunk_view(
    app: &App,
    path: &str,
    index: usize,
    hunk: &crate::hunks::Hunk,
    focused: bool,
) -> NodeBuilder {
    let model = app.model.get();
    let note = app.hunk_note(path, &hunk.id);
    let (verdict, tone) = match note.as_ref().and_then(|note| note.verdict) {
        Some(crate::hunks::Verdict::Accepted) => ("accepted", Tone::Success),
        Some(crate::hunks::Verdict::Rejected) => ("rejected", Tone::Warn),
        None => ("pending", Tone::Neutral),
    };
    let editing = app
        .note_target
        .peek()
        .is_some_and(|(target, id)| target == path && id == hunk.id);
    let menu = app.clone();
    let mut card = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(4.0))
        .child(
            list_row(focused, hunk.header.clone(), String::new())
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .height(24.0)
                        .padding_x(8.0)
                        .gap(6.0)
                        .align_items(AlignItems::Center),
                )
                .child(badge(verdict, tone))
                .child(spacer())
                .pointer_down(move |event| {
                    if event.button == Some(MouseButton::Right) {
                        menu.hunk_context(index);
                    } else {
                        menu.hunk_cursor.set(index);
                    }
                    event.stop_propagation();
                }),
        )
        .child(crate::tui::sigil_view::chat::entry::patch_view(
            app,
            hunk.patch.clone(),
            model.prefs.diff_layout,
        ));
    drop(model);
    if editing {
        let commit = app.clone();
        let edit = app.clone();
        card = card.child(node(move |cx: &mut Cx| {
            let id = cx.add(
                text_edit(edit.note_text)
                    .caret_position(edit.note_caret)
                    .submit(move |text| commit.note_commit(text))
                    .layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 20.0))
                    .class(classes!("font-mono text-xs bg-gray-1 rounded-md")),
            );
            request_focus(id);
            id
        }));
        card = card.child(
            label("ctrl+enter saves · esc cancels · sent with your next prompt".to_string())
                .class(classes!("text-xs text-gray-11")),
        );
    } else if let Some(comment) = note.and_then(|note| note.comment) {
        card = card.child(
            label(format!("note: {comment}"))
                .class(classes!("text-xs text-info-11"))
                .layout(LayoutStyle::block().fill_width().padding_x(8.0)),
        );
    }
    card
}
