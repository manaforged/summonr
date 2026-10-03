use sigil::prelude::Paint;
use sigil::prelude::{MouseButton, NodeBuilder, NodeEventsExt, Tone, chip, list_row};

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
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        );
    }
    for (index, (record, status)) in model.jobs.iter().enumerate() {
        let menu = app.clone();
        let toggle = app.clone();
        let tone = match status.state {
            crate::jobs::JobState::Running => Tone::Info,
            crate::jobs::JobState::Killed => Tone::Warn,
            crate::jobs::JobState::Exited(0) => Tone::Success,
            crate::jobs::JobState::Exited(_) => Tone::Error,
            crate::jobs::JobState::Dead => Tone::Neutral,
        };
        column = column.child(
            list_row(index == cursor, record.command.clone(), status.label())
                .child(chip(status.label(), tone))
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
