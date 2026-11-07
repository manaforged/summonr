use sigil::prelude::Space;
use sigil::prelude::{
    ControlSize, LayoutStyle, NodeBuilder, Token, Widget, button, container, virtual_list,
};

use super::{App, StatusView, canvas, children, entry, peers, text};
use crate::panes::ledger::Mark;
use summonr_agent::mail::Presence;

const STATE_COLS: usize = 8;

#[derive(Clone, PartialEq)]
pub(super) struct Row {
    pub(super) id: String,
    title: String,
    model: String,
    summary: String,
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
                model: String::new(),
                summary: String::new(),
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
            title: if !agent.name.is_empty() {
                agent.name.clone()
            } else if !agent.title.is_empty() {
                agent.title.clone()
            } else {
                format!("Agent {}", agent.id)
            },
            model: agent.model.clone(),
            summary: entry::workspace_relative(
                if agent.last.is_empty() {
                    &agent.assignment
                } else {
                    &agent.last
                },
                &agent.roots(&app.roots()),
            )
            .lines()
            .next()
            .unwrap_or_default()
            .to_string(),
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
            detail: entry::workspace_relative(&agent.last, &agent.roots(&app.roots())),
            meta: format!(
                "{} · {} · {} tok\nModel: {}\nTask: {}\nWorkspace: {}\nChild: {}",
                agent.access.name(),
                agent.clock(),
                summonr_agent::provider::compact(agent.tokens),
                if agent.model.is_empty() {
                    "unknown"
                } else {
                    &agent.model
                },
                agent.assignment,
                if agent.workspace.is_empty() {
                    "unknown".to_string()
                } else {
                    entry::workspace_relative(&agent.workspace, &app.roots())
                },
                agent.id
            ),
            clear: agent.background,
        })
        .chain(peers(&model).map(|peer| Row {
            id: peer.id.clone(),
            title: peer.label().to_string(),
            model: peer.model.clone(),
            summary: peer.tool.clone(),
            state: peer.status.to_string(),
            running: peer.status == Presence::Working,
            failed: peer.status == Presence::Unknown,
            mark: if peer.status == Presence::Unknown {
                '!'
            } else {
                '○'
            },
            detail: peer.tool.clone(),
            meta: format!(
                "Model: {}\nWorkspace: {}\nPID: {}\nID: {}",
                peer.model,
                entry::workspace_relative(&peer.cwd, &app.roots()),
                peer.pid,
                peer.id,
            ),
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
                    let column = width.saturating_sub(STATE_COLS + 11);
                    let model = crate::cells::ellipsize(&row.model, width / 3);
                    let title = crate::cells::ellipsize(
                        &row.title,
                        column.saturating_sub(sigil::app::cell_width(&model) + 3),
                    );
                    let title = if model.is_empty() {
                        title.to_string()
                    } else {
                        format!("{title} · {model}")
                    };
                    let pad = column.saturating_sub(sigil::app::cell_width(&title));
                    let title = format!("{title}{}", " ".repeat(pad));
                    let mut body = container()
                        .layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0))
                        .child(
                            container()
                                .layout(LayoutStyle::flex_row().fill_width().gap(Space::Sm))
                                .child(marker)
                                .child(
                                    button(title)
                                        .ghost()
                                        .size(ControlSize::Compact)
                                        .accessible(format!(
                                            "{}; {}; {}",
                                            row.title, row.model, row.state
                                        ))
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
                    if !expanded && !row.summary.is_empty() {
                        body = body.child(
                            container()
                                .layout(LayoutStyle::block().padding_left(Space::Xl))
                                .child(text(
                                    crate::cells::ellipsize(&row.summary, width.saturating_sub(4)),
                                    Token::Gray10,
                                )),
                        );
                    }
                    if expanded {
                        let mut detail = container()
                            .layout(
                                LayoutStyle::flex_column()
                                    .fill_width()
                                    .padding_left(Space::Xl),
                            )
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
