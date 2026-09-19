use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, NodeStyleExt, Role, classes,
    container, label, modal, signal, virtual_list,
};

use super::super::app::App;
use super::{Row, setting_rows};

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    container().layout(LayoutStyle::block()).child(
        modal(app.settings_open)
            .title("⚙ options")
            .trap(true)
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_column()
                            .gap(4.0)
                            .fill_width()
                            .fill_height(),
                    )
                    .children_reactive(move || {
                        super::super::view::watch(&paint);
                        let rows = setting_rows(&paint);
                        let selected = paint.settings_sel.get().min(rows.len().saturating_sub(1));
                        let items = headings(&rows);
                        let lookup = rows.clone();
                        let source = paint.clone();
                        vec![
                            container().layout(LayoutStyle::block().fill_width()).child(
                                virtual_list(
                                    items,
                                    move |_, (heading, index): &(Option<&'static str>, usize)| {
                                        entry(
                                            source.clone(),
                                            &lookup[*index],
                                            *heading,
                                            *index,
                                            *index == selected,
                                        )
                                    },
                                )
                                .row_height(22.0)
                                .viewport_fill()
                                .scroll_into_view(selected),
                            ),
                        ]
                    }),
            ),
    )
}

fn headings(rows: &[Row]) -> Vec<(Option<&'static str>, usize)> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let heading = (index == 0 || rows[index - 1].group != row.group).then_some(row.group);
            (heading, index)
        })
        .collect()
}

fn entry(
    app: App,
    row: &Row,
    heading: Option<&'static str>,
    index: usize,
    active: bool,
) -> NodeBuilder {
    let choose = app.clone();
    let mut node = container()
        .role(Role::Button)
        .label(row.label.clone())
        .focusable(true)
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(22.0)
                .padding_x(8.0)
                .gap(8.0),
        )
        .class(classes!("rounded-md"))
        .class_when(signal(active), "bg-gray-5")
        .pointer_down(move |event| {
            if choose.settings_sel.peek() == index {
                choose.adjust_setting();
            } else {
                choose.settings_sel.set(index);
            }
            event.stop_propagation();
        });
    if let Some(heading) = heading {
        node = node.child(label(heading.to_string()).class(classes!("text-xs text-gray-11")));
    } else {
        node = node.child(container().layout(LayoutStyle::block().width(96.0)));
    }
    node.child(label(row.label.clone()).class(classes!("text-sm text-gray-12")))
        .child(
            label(row.value.clone())
                .ellipsize()
                .class(classes!("text-sm text-gray-11")),
        )
}
