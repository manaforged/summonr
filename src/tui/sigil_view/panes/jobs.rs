use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, NodeStyleExt, Tone,
    badge, container, list_row,
};

use super::App;
use super::body::header;

pub(super) fn jobs(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.get();
    let open = app.job_open.peek();
    let cursor = app.job_cursor.peek();
    column = column.child(header(
        &app,
        "Jobs".to_string(),
        model.jobs.len().to_string(),
    ));
    if model.jobs.is_empty() {
        return column.child(
            sigil::kernel::text("no background commands — run_command background:true starts one")
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .class("text-xs text-gray-11"),
        );
    }
    for (index, (record, status)) in model.jobs.iter().enumerate() {
        let menu = app.clone();
        let toggle = app.clone();
        let tone = if status.running {
            Tone::Info
        } else if status.killed {
            Tone::Warn
        } else {
            match status.exit_code {
                Some(0) => Tone::Success,
                Some(_) => Tone::Error,
                None => Tone::Neutral,
            }
        };
        column = column.child(
            list_row(index == cursor, record.command.clone(), status.label())
                .child(badge(status.label(), tone))
                .pointer_down(move |event| {
                    if event.button == Some(MouseButton::Right) {
                        menu.context_row(index);
                    } else {
                        toggle.job_cursor.set(index);
                        toggle.job_enter();
                    }
                    event.stop_propagation();
                }),
        );
        if open.as_deref() == Some(record.id.as_str()) {
            let mut detail = container()
                .layout(LayoutStyle::flex_column().fill_width().gap(1.0))
                .class("rounded-md bg-gray-1");
            for line in status.text.lines() {
                detail = detail.child(
                    sigil::kernel::text(line.to_string())
                        .nowrap()
                        .class(sigil::style::Token::OnSurface.text())
                        .ellipsize()
                        .layout(
                            LayoutStyle::block()
                                .fill_width()
                                .min_size(0.0, 14.0)
                                .padding_x(8.0),
                        )
                        .class("text-xs font-mono text-gray-11"),
                );
            }
            column = column.child(detail);
        }
    }
    column
}
