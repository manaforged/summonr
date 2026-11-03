use sigil::prelude::{
    ControlSize, LayoutStyle, NodeBuilder, Token, Widget, button, container, virtual_list,
};

use super::{App, StatusView, canvas, children, entry, peers, text};
use crate::mail::Presence;
use crate::tui::sigil_view::panes::ledger::Mark;

#[derive(Clone, PartialEq)]
pub(super) struct Row {
    pub(super) id: String,
    title: String,
    state: String,
    running: bool,
    failed: bool,
    mark: char,
    detail: String,
    meta: String,
    clear: bool,
}

pub(super) fn rows(app: &App) -> Vec<Row> {
    let model = app.model.get();
    if model.status_view == StatusView::Tasks {
        return model
            .ledger
            .iter()
            .enumerate()
            .map(|(index, row)| Row {
                id: index.to_string(),
                title: row.text.clone(),
                state: match row.mark {
                    Mark::Done => "Done",
                    Mark::Active => "Working",
                    Mark::Pending => "Pending",
                    Mark::Blocked => "Blocked",
                    Mark::Plain => "",
                }
                .to_string(),
                running: row.mark == Mark::Active,
                failed: row.mark == Mark::Blocked,
                mark: row.mark.glyph(),
                detail: row.shown(),
                meta: String::new(),
                clear: false,
            })
            .collect();
    }
    children(&model)
        .map(|agent| Row {
            id: agent.id.clone(),
            title: agent.title.clone(),
            state: if agent.failed {
                "Failed"
            } else if agent.done {
                "Done"
            } else {
                "Working"
            }
            .to_string(),
            running: !agent.done,
            failed: agent.failed,
            mark: if agent.failed {
                '!'
            } else if agent.done {
                '✓'
            } else {
                '○'
            },
            detail: entry::workspace_relative(&agent.last, &agent.roots(&app.roots)),
            meta: format!(
                "{} · {} · {} tok\n{}",
                agent.access.name(),
                agent.clock(),
                crate::provider::compact(agent.tokens),
                agent.assignment
            ),
            clear: agent.background,
        })
        .chain(peers(&model).map(|peer| Row {
            id: peer.id.clone(),
            title: peer.label().to_string(),
            state: peer.status.to_string(),
            running: peer.status == Presence::Working,
            failed: peer.status == Presence::Unknown,
            mark: if peer.status == Presence::Unknown {
                '!'
            } else {
                '○'
            },
            detail: peer.tool.clone(),
            meta: peer.model.clone(),
            clear: true,
        }))
        .collect()
}

pub(super) fn list(app: App) -> NodeBuilder {
    let update = app.clone();
    let view = sigil::reactive::derived(move || {
        update.ticks.with(|_| ());
        rows(&update)
    });
    let empty = sigil::reactive::derived(move || view.with(Vec::is_empty));
    let list = app.status_list;
    let length = sigil::reactive::derived(move || view.with(Vec::len));
    sigil::reactive::effect(move || {
        let total = length.get();
        if list.cursor.peek() >= total {
            sigil::reactive::defer(move || list.select(total.saturating_sub(1)));
        }
    });
    let shown = canvas::overlay_shown(usize::MAX, app.viewport.peek())
        .saturating_sub(4)
        .max(1);
    let height = shown as f32 * canvas::overlay_list_row();
    let width = super::Size::Reading
        .cells(app.viewport.peek())
        .saturating_sub(8);
    let current = app.clone();
    let expanded = app.status_detail;
    container().children_reactive(move || {
        if empty.get() {
            return vec![container().child(text("Nothing to show yet", Token::Gray10))];
        }
        let app = current.clone();
        let cursor = list.cursor;
        let expanded = expanded.get();
        vec![
            container().child(
                virtual_list(view, move |index, row: &Row| {
                    let open = app.clone();
                    let id = row.id.clone();
                    let selected = cursor.get() == index;
                    let expanded = expanded.as_ref() == Some(&row.id);
                    let marker = if row.running {
                        entry::loading_mark(app.ticks)
                    } else {
                        text(
                            row.mark.to_string(),
                            if row.failed {
                                Token::Error11
                            } else {
                                Token::Gray10
                            },
                        )
                    };
                    let mut body = container()
                        .layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0))
                        .child(
                            container()
                                .layout(LayoutStyle::flex_row().fill_width().gap(8.0))
                                .child(marker)
                                .child(
                                    button(crate::display::ellipsize(
                                        &row.title,
                                        width
                                            .saturating_sub(sigil::app::cell_width(&row.state) + 8),
                                    ))
                                    .ghost()
                                    .size(ControlSize::Compact)
                                    .accessible(format!("{}; {}", row.title, row.state))
                                    .activate(move || {
                                        open.status_list.select(index);
                                        open.status_detail.update(|shown| {
                                            *shown = if shown.as_ref() == Some(&id) {
                                                None
                                            } else {
                                                Some(id.clone())
                                            };
                                        });
                                    }),
                                )
                                .child(text(
                                    row.state.clone(),
                                    if row.failed {
                                        Token::Error11
                                    } else {
                                        Token::Gray10
                                    },
                                ))
                                .child(text(
                                    if expanded {
                                        "▾"
                                    } else if selected {
                                        "›"
                                    } else {
                                        "▸"
                                    },
                                    Token::Gray10,
                                )),
                        );
                    if expanded {
                        let mut detail = container()
                            .layout(LayoutStyle::flex_column().fill_width().padding_left(24.0))
                            .child(text(row.meta.clone(), Token::Gray10).wrap())
                            .child(text(row.detail.clone(), Token::Gray12).wrap());
                        if row.clear {
                            let clear = app.clone();
                            let id = row.id.clone();
                            detail = detail.child(
                                button("Clear")
                                    .ghost()
                                    .size(ControlSize::Compact)
                                    .accessible(format!("Clear subagent {}", row.title))
                                    .activate(move || clear.clear_agent(&id)),
                            );
                        }
                        body = body.child(detail);
                    }
                    body
                })
                .measured()
                .estimate(canvas::overlay_list_row())
                .height(height)
                .viewport_height(height)
                .scroll_bind(list.scroll)
                .follow_row(cursor),
            ),
        ]
    })
}
