use sigil::prelude::Paint;
use sigil::prelude::{LayoutBuilderExt, LayoutStyle, NodeBuilder, Tone, container, notice_band};

use crate::tui::sigil_view::ui::{Action, ActionKind, Hints, Key, Prompt};

use super::App;
use super::busy_status;

pub(in crate::tui::sigil_view) use super::strip::{agents, rows as strip_rows};

#[derive(Clone, PartialEq)]
pub(in crate::tui::sigil_view) enum Band {
    Approval { detail: String, persist: bool },
    Form { lines: Vec<String>, review: bool },
    Busy { status: String, attached: usize },
    Attached(usize),
}

pub(in crate::tui::sigil_view) fn band(app: &App) -> Option<Band> {
    if let Some((detail, persist)) = app.approval.get() {
        return Some(Band::Approval { detail, persist });
    }
    if let Some(state) = app.form.get() {
        return Some(Band::Form {
            lines: state.lines(),
            review: state.stage == crate::tui::sigil_view::app::Stage::Review,
        });
    }
    let attached = app.attached.get();
    if let Some(status) = busy_status(&app.model.get(), app.ticks.get()) {
        return Some(Band::Busy { status, attached });
    }
    (attached > 0).then_some(Band::Attached(attached))
}

pub(in crate::tui::sigil_view) fn attached_text(attached: usize) -> String {
    let noun = if attached == 1 { "image" } else { "images" };
    format!("{attached} {noun} attached")
}

pub(in crate::tui::sigil_view) fn attached_note(app: &App, attached: usize) -> String {
    format!(
        "{} · {}",
        attached_text(attached),
        Hints::new()
            .key(Key::Enter, "sends with your prompt")
            .text(&app.model.peek().prefs.hotkeys)
    )
}

pub(super) fn band_nodes(app: &App, width: usize, band: Band) -> NodeBuilder {
    match band {
        Band::Approval { detail, persist } => {
            above(approval_band(app.clone(), width, detail, persist))
        }
        Band::Form { lines, review } => {
            above(super::form::form_band(app.clone(), width, lines, review))
        }
        Band::Busy { status, attached } => {
            let row = busy_row(&crate::text::ellipsize(&status, width));
            if attached == 0 {
                row
            } else {
                container()
                    .layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0))
                    .child(row)
                    .child(band_nodes(app, width, Band::Attached(attached)))
            }
        }
        Band::Attached(attached) => above(notice_band(attached_note(app, attached), Tone::Info)),
    }
}

pub(super) fn bands(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let active = sigil::reactive::derived(move || band(&update));
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            active
                .get()
                .map(|band| vec![band_nodes(&app, width, band)])
                .unwrap_or_default()
        })
}

fn busy_row(status: &str) -> NodeBuilder {
    let (spinner, rest) = status.split_at(status.find(' ').unwrap_or(status.len()));
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .child(
            sigil::kernel::text(spinner.to_owned())
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Accent),
        )
        .child(
            sigil::kernel::text(rest.to_owned())
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        )
}

fn above(child: NodeBuilder) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_column().fill_width().padding_top(8.0))
        .child(child)
}

pub(in crate::tui::sigil_view) fn approval_band(
    app: App,
    width: usize,
    detail: String,
    persist: bool,
) -> NodeBuilder {
    use crate::runtime::GrantAnswer;
    let mut hints = Hints::new().key(Key::Fixed("y"), "allow");
    if persist {
        hints = hints.key(Key::Fixed("Y"), "always");
    }
    hints = hints.key(Key::Fixed("n"), "deny");
    let keys = app.model.peek().prefs.hotkeys.clone();
    let answer = |kind: GrantAnswer| {
        let app = app.clone();
        move || app.approval_answer(kind)
    };
    let mut prompt = approval_lines(
        Prompt::new(Tone::Warn, "Needs approval", hints),
        &detail,
        width,
    )
    .action(Action::new(
        "Allow",
        ActionKind::Primary,
        answer(GrantAnswer::Allow),
    ));
    if persist {
        prompt = prompt.action(Action::new(
            "Always",
            ActionKind::Plain,
            answer(GrantAnswer::Always),
        ));
    }
    prompt
        .action(Action::new(
            "Deny",
            ActionKind::Destructive,
            answer(GrantAnswer::Deny),
        ))
        .build(width, &keys)
}

const APPROVAL_LINES: usize = 12;

fn approval_lines(prompt: Prompt, detail: &str, width: usize) -> Prompt {
    let mut lines = crate::text::lines_within(detail, width.saturating_sub(4));
    if lines.len() > APPROVAL_LINES {
        lines.truncate(APPROVAL_LINES);
        if let Some(last) = lines.last_mut() {
            last.push_str(" …");
        }
    }
    lines.into_iter().fold(prompt, Prompt::line)
}
