use super::*;

pub(super) fn row(entry: Entry, tools: ToolOutput) -> NodeBuilder {
    match entry {
        Entry::User(body) => container()
            .layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .justify_content(JustifyContent::End),
            )
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_row()
                            .padding_x(14.0)
                            .padding_y(8.0)
                            .max_size(560.0, f32::INFINITY),
                    )
                    .class(classes!("rounded-lg bg-gray-4"))
                    .child(text(body).class(classes!("text-sm text-gray-12"))),
            ),
        Entry::Assistant { text: body, .. } => prose(&body, "text-sm text-gray-12"),
        Entry::Thinking(body) => prose(&body, "text-xs text-gray-11"),
        Entry::Receipt(body) => prose(&body, "text-xs text-gray-11"),
        Entry::Tool {
            name, arg, output, ..
        } => container()
            .layout(LayoutStyle::flex_column().fill_width().gap(4.0))
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_row()
                            .gap(8.0)
                            .align_items(AlignItems::Center),
                    )
                    .child(badge(name, Tone::Neutral))
                    .child(
                        label(arg)
                            .ellipsize()
                            .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 14.0))
                            .class(classes!("text-xs font-mono text-gray-11")),
                    ),
            )
            .child(text(tools.text(&output)).class(classes!("text-xs font-mono text-gray-11"))),
        Entry::Verify { command, passed } => wrap(
            label(format!("{} {command}", if passed { "✓" } else { "✗" })).class(if passed {
                classes!("text-xs text-success-11")
            } else {
                classes!("text-xs text-error-11")
            }),
        ),
        Entry::Mail { from, text: body } => {
            prose(&format!("{from}: {body}"), "text-sm text-info-11")
        }
        Entry::Aborted(reason) | Entry::Error(reason) => {
            wrap(text(reason).class(classes!("text-xs text-error-11")))
        }
        Entry::System(body) => wrap(text(body).class(classes!("text-xs text-gray-11"))),
        Entry::Chart(rounds) => wrap(
            chart(rounds.iter().map(|round| round.total() as f32))
                .height(72.0)
                .width(280.0),
        ),
        Entry::Image {
            source,
            label: name,
        } => {
            let height = source.height() as f32;
            let width = source.width() as f32;
            container()
                .layout(LayoutStyle::flex_column().fill_width().gap(6.0))
                .child(
                    image(source)
                        .alt(name.clone())
                        .fit(ObjectFit::Contain)
                        .layout(
                            LayoutStyle::block()
                                .fill_width()
                                .max_size(width, height)
                                .height(height),
                        ),
                )
                .child(label(name).class(classes!("text-xs text-gray-11")))
        }
    }
}

pub(super) fn wrap(child: impl IntoNode + 'static) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fill_width().min_size(0.0, 0.0))
        .child(child)
}

pub(super) fn prose(body: &str, class: &'static str) -> NodeBuilder {
    let mut column = container().layout(LayoutStyle::flex_column().fill_width().gap(6.0));
    for para in body.split('\n') {
        let mut row = container()
            .layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .gap(4.0)
                    .align_items(AlignItems::Center)
                    .min_size(0.0, 0.0),
            )
            .class(classes!("flex-wrap"));
        let mut rest = para;
        while let Some(start) = rest.find('`') {
            if start > 0 {
                row = row.child(text(rest[..start].to_string()).class(class));
            }
            rest = &rest[start + 1..];
            let Some(end) = rest.find('`') else {
                break;
            };
            row = row.child(badge(rest[..end].to_string(), Tone::Neutral));
            rest = &rest[end + 1..];
        }
        if !rest.is_empty() {
            row = row.child(text(rest.to_string()).class(class));
        }
        column = column.child(row);
    }
    column
}
