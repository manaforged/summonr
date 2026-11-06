use sigil::prelude::{LayoutStyle, NodeBuilder, Tone, container, notice_band};

use crate::ui::{Action, ActionKind, Hints, Key, Prompt};

use super::App;
use sigil::prelude::Widget;

pub(crate) use super::strip::{bar as status_bar, rows as status_rows};

#[derive(Clone, PartialEq)]
pub(crate) enum Band {
    Approval { detail: String, persist: bool },
    Form { lines: Vec<String>, review: bool },
    Attached(usize),
}

pub(crate) fn band(app: &App) -> Option<Band> {
    if let Some(crate::app::Asked {
        detail, persist, ..
    }) = app.approval.get()
    {
        return Some(Band::Approval { detail, persist });
    }
    if let Some(state) = app.form.get() {
        return Some(Band::Form {
            lines: state.shown(&app.draft.get()),
            review: state.stage == crate::app::Stage::Review,
        });
    }
    let attached = app.attached.get();
    (attached > 0).then_some(Band::Attached(attached))
}

pub(crate) fn attached_text(attached: usize) -> String {
    let noun = if attached == 1 { "image" } else { "images" };
    format!("{attached} {noun} attached")
}

pub(crate) fn attached_note(app: &App, attached: usize) -> String {
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

fn above(child: NodeBuilder) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_column().fill_width().padding_top(8.0))
        .child(child)
}

pub(crate) fn approval_band(app: App, width: usize, detail: String, persist: bool) -> NodeBuilder {
    use summonr_agent::runtime::GrantAnswer;
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
        Key::Fixed("y"),
        ActionKind::Primary,
        answer(GrantAnswer::Allow),
    ));
    if persist {
        prompt = prompt.action(Action::new(
            "Always",
            Key::Fixed("Y"),
            ActionKind::Plain,
            answer(GrantAnswer::Always),
        ));
    }
    prompt
        .action(Action::new(
            "Deny",
            Key::Fixed("n"),
            ActionKind::Destructive,
            answer(GrantAnswer::Deny),
        ))
        .build(width, &keys)
}

const APPROVAL_LINES: usize = 12;

fn approval_lines(prompt: Prompt, detail: &str, width: usize) -> Prompt {
    let mut lines = crate::cells::lines_within(detail, width.saturating_sub(4));
    if lines.len() > APPROVAL_LINES {
        lines.truncate(APPROVAL_LINES);
        if let Some(last) = lines.last_mut() {
            last.push_str(" …");
        }
    }
    lines.into_iter().fold(prompt, Prompt::line)
}
