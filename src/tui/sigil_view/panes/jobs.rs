use sigil::prelude::{
    MouseButton, NodeBuilder, NodeEventsExt, NodeStyleExt, Tone, badge, list_row,
};

use super::App;
use super::body::{detail, header};

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
            column = column.child(detail(status.text.lines().map(str::to_string)));
        }
    }
    column
}
