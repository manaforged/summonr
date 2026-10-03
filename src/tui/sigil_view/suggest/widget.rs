use super::{App, LIMIT, MAX_WIDTH, canvas, mention, usage};
use crate::tui::sigil_view::prelude::{group, hint, pick};
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::MouseButton;
use sigil::prelude::NodeBuilder;
use sigil::prelude::NodeEventsExt;
use sigil::prelude::Paint;
use sigil::prelude::container;

#[derive(Clone, PartialEq)]
struct SuggestRow {
    index: usize,
    name: String,
    description: String,
    selected: bool,
}

#[derive(Clone, PartialEq)]
struct Popup {
    title: String,
    rows: Vec<SuggestRow>,
    keys: &'static str,
    count: String,
}

fn popup(app: &App, width: usize) -> Option<Popup> {
    let draft = app.draft.get();
    let caret = app.caret.get();
    let rows = app.viewport.get().1;
    let model = app.model.get();
    if app.view_edit.get() || !app.commands.open(&draft, caret, &model) || width < 12 {
        return None;
    }
    let matches = app.commands.rows(&draft, caret, &model);
    let cursor = app.commands.cursor(matches.len());
    let limit = matches
        .len()
        .min(LIMIT)
        .min(usize::from(rows / 2).saturating_sub(2));
    if limit == 0 {
        return None;
    }
    let start = cursor.saturating_add(1).saturating_sub(limit);
    let shown = &matches[start..matches.len().min(start + limit)];
    let head = app.commands.head(&draft);
    let title = match (mention(&draft, caret), head) {
        (Some(_), _) => "mentions".to_string(),
        (None, Some(head)) => format!("{head} {}", usage(head)),
        (None, None) => "commands".to_string(),
    };
    let rows = shown
        .iter()
        .enumerate()
        .map(|(offset, (name, description))| {
            let index = start + offset;
            let label = match head {
                Some(head) => name.trim_start_matches(head).trim_start().to_string(),
                None => format!("{name} {}", usage(name)).trim_end().to_string(),
            };
            SuggestRow {
                index,
                name: label,
                description: description.clone(),
                selected: index == cursor,
            }
        })
        .collect();
    let keys = if head.is_some() || mention(&draft, caret).is_some() {
        "↑↓ · tab complete · ↵ complete · esc"
    } else {
        "↑↓ · tab complete · ↵ run · esc"
    };
    let count = format!("{} / {}", cursor + 1, matches.len());
    Some(Popup {
        title,
        rows,
        keys,
        count,
    })
}

pub(in crate::tui::sigil_view) fn widget(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let popup = sigil::reactive::derived(move || popup(&update, width));
    container()
        .layout(LayoutStyle::flex_column().fill_width().shrink(0.0))
        .children_reactive(move || {
            let Some(popup) = popup.get() else {
                return Vec::new();
            };
            let popup_width = MAX_WIDTH.min(width.saturating_sub(4).max(1));
            let mut body = container()
                .layout(
                    LayoutStyle::flex_column()
                        .width(popup_width as f32 * canvas::cell().w)
                        .gap(0.0),
                )
                .child(group(popup.title));
            for row in popup.rows {
                let click = app.clone();
                body = body.child(
                    pick(row.selected, row.name, row.description.as_str()).pointer_down(
                        move |event| {
                            if event.button == Some(MouseButton::Left) {
                                click.commands.cursor.set(row.index);
                                click.accept();
                                event.stop_propagation();
                            }
                        },
                    ),
                );
            }
            vec![
                container()
                    .layout(LayoutStyle::flex_column().padding_x(8.0).gap(0.0))
                    .border_w(canvas::cell().h as i16)
                    .border_color(sigil::prelude::Token::Gray6)
                    .bg(canvas::palette().tool_bg)
                    .child(body.child(hint(popup.keys, popup.count))),
            ]
        })
}
