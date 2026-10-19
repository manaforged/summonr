use sigil::prelude::{MouseButton, NodeBuilder, NodeEventsExt, Tone, chip, list_row};

use super::App;
use super::body::{Empty, detail, empty, header};

const NO_JOBS: Empty = Empty {
    window: "Commands the agent leaves running in the background show here, with their output and exit status.",
    terminal: "no background commands; run_command background:true starts one",
};

pub(super) fn jobs(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.get();
    let open = app.job_open.peek();
    let cursor = app.job_cursor.peek();
    column = column.child(header(
        &app,
        "Jobs".to_string(),
        super::body::count(model.jobs.len()),
    ));
    if model.jobs.is_empty() {
        return column.child(empty(&NO_JOBS));
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
            list_row(index == cursor, record.command.clone(), String::new())
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
