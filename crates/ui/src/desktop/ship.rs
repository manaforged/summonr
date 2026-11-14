use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, ControlSize, NodeBuilder, Signal, Tone, Widget, button, derived, effect,
};
use sigil::reactive::untrack;
use summonr_agent::forge::{Ask, Bucket, Check, Failure, Pull, Review, State};

use super::App;
use super::style::{Text, XS, text};

const OPEN_PR: &str = "Open PR";
const OPEN_PR_KEY: &str = "⌘⇧P";
const MERGE: &str = "Merge";
const CHECKS: &str = "Checks";
const PULL: &str = "Pull request";
const UNTITLED: &str = "Changes";

/// What the checks chip says, in the order it wins.
const VERDICTS: [(Verdict, &str, Tone); 4] = [
    (Verdict::Failed, "Checks failed", Tone::Error),
    (Verdict::Changes, "Changes requested", Tone::Warning),
    (Verdict::Running, "Checks running", Tone::Neutral),
    (Verdict::Passed, "Checks passed", Tone::Success),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Failed,
    Changes,
    Running,
    Passed,
}

/// The header's ship controls: Open PR, then the pull request, its checks and Merge.
pub(super) fn view(app: &App) -> NodeBuilder {
    watch(app);
    let model = app.model;
    let changed = derived(move || model.with(|model| !model.diff.files.is_empty()));
    let asking = app.forge.asking;
    let held = derived(move || asking.get() || model.with(|model| model.busy_since.is_some()));
    let shown = app.clone();
    sigil::prelude::row()
        .gap(XS)
        .shrink(0.0)
        .align_items(AlignItems::Center)
        .children_reactive(move || items(&shown, changed.get(), held))
}

/// Opens or finds the pull request for this pane's topic.
pub(super) fn open(app: &App) {
    if !working(app) {
        app.forge_ask(Ask::Open(title(app)));
    }
}

/// Merges this pane's pull request when one is open.
pub(super) fn merge(app: &App) {
    if !working(app) && open_pull(app).is_some() {
        app.forge_ask(Ask::Merge);
    }
}

fn items(app: &App, changed: bool, held: Signal<bool>) -> Vec<NodeBuilder> {
    match app.forge.state.get() {
        Some(Err(error @ (Failure::SignedOut | Failure::TimedOut { .. }))) => {
            vec![text(Text::Meta, error.to_string()).nowrap().into()]
        }
        Some(Ok(None)) if changed => vec![open_button(app, held)],
        Some(Ok(Some(pull))) if pull.state == State::Open => pull_items(app, &pull, held),
        Some(Ok(_) | Err(_)) | None => Vec::new(),
    }
}

fn open_button(app: &App, held: Signal<bool>) -> NodeBuilder {
    let press = app.clone();
    let button = button(OPEN_PR)
        .icon(Icon::GitPullRequest)
        .size(ControlSize::Compact)
        .accessible(OPEN_PR)
        .disabled(held)
        .activate(move || open(&press));
    super::tip(OPEN_PR, Some(OPEN_PR_KEY), button.into())
}

fn pull_items(app: &App, pull: &Pull, held: Signal<bool>) -> Vec<NodeBuilder> {
    let url = pull.url.clone();
    let link = button(format!("#{}", pull.number))
        .ghost()
        .size(ControlSize::Compact)
        .accessible(PULL)
        .activate(move || drop(sigil::app::open(&url)));
    let mut items = vec![super::tip(PULL, None, link.into())];
    if let Some(verdict) = verdict(pull) {
        items.push(checks_chip(app, pull, verdict));
    }
    let press = app.clone();
    let merge_button = button(MERGE)
        .size(ControlSize::Compact)
        .accessible(MERGE)
        .disabled(held)
        .activate(move || merge(&press));
    items.push(merge_button.into());
    items
}

fn checks_chip(app: &App, pull: &Pull, verdict: Verdict) -> NodeBuilder {
    let (_, label, tone) = VERDICTS
        .iter()
        .find(|row| row.0 == verdict)
        .copied()
        .unwrap_or(VERDICTS[3]);
    let (press, pull) = (app.clone(), pull.clone());
    button(label)
        .ghost()
        .tone(tone)
        .size(ControlSize::Compact)
        .accessible(CHECKS)
        .activate(move || match prompt(&pull) {
            Some(prompt) => press.send_text(&prompt),
            None => press.forge_ask(Ask::Status),
        })
        .into()
}

fn verdict(pull: &Pull) -> Option<Verdict> {
    let has = |wanted: &[Bucket]| {
        pull.checks
            .iter()
            .any(|check| wanted.contains(&check.bucket))
    };
    if has(&[Bucket::Fail, Bucket::Cancel]) {
        Some(Verdict::Failed)
    } else if pull.review == Review::ChangesRequested {
        Some(Verdict::Changes)
    } else if has(&[Bucket::Pending]) {
        Some(Verdict::Running)
    } else {
        (!pull.checks.is_empty()).then_some(Verdict::Passed)
    }
}

fn prompt(pull: &Pull) -> Option<String> {
    let failed: Vec<&Check> = pull
        .checks
        .iter()
        .filter(|check| matches!(check.bucket, Bucket::Fail | Bucket::Cancel))
        .collect();
    if !failed.is_empty() {
        let lines: Vec<String> = failed
            .iter()
            .map(|check| format!("- {}: {}", check.name, check.link))
            .collect();
        return Some(format!(
            "These checks failed on pull request #{} ({}):\n{}\nFind the cause and fix it.",
            pull.number,
            pull.url,
            lines.join("\n")
        ));
    }
    (pull.review == Review::ChangesRequested).then(|| {
        format!(
            "Pull request #{} ({}) has changes requested. Read the review with `gh pr view {} --comments` and make the changes.",
            pull.number, pull.url, pull.number
        )
    })
}

fn watch(app: &App) {
    let model = app.model;
    let moment =
        derived(move || model.with(|model| (model.session.clone(), model.busy_since.is_none())));
    let ask = app.clone();
    effect(move || {
        let (session, idle) = moment.get();
        if idle && !session.is_empty() {
            untrack(|| ask.forge_ask(Ask::Status));
        }
    });
    let state = app.forge.state;
    let shelve = app.clone();
    effect(move || {
        if matches!(state.get(), Some(Ok(Some(pull))) if pull.state == State::Merged) {
            untrack(|| super::shelf::archive(&topic(&shelve)));
        }
    });
}

fn working(app: &App) -> bool {
    app.model.with(|model| model.busy_since.is_some())
}

fn open_pull(app: &App) -> Option<Pull> {
    match app.forge.state.peek() {
        Some(Ok(Some(pull))) if pull.state == State::Open => Some(pull),
        Some(Ok(_) | Err(_)) | None => None,
    }
}

fn topic(app: &App) -> String {
    let session = app.model.with(|model| model.session.clone());
    app.sessions
        .all_chats()
        .iter()
        .find(|info| info.id == session)
        .map_or(session, |info| super::topic::key(info).to_owned())
}

fn title(app: &App) -> String {
    let topic = topic(app);
    let root = app
        .sessions
        .all_chats()
        .into_iter()
        .find(|info| info.id == topic)
        .map(|info| super::sidebar::title(&info));
    root.or_else(|| {
        app.model.with(|model| {
            model
                .live
                .iter()
                .find(|live| live.session == topic && !live.title.is_empty())
                .map(|live| live.title.clone())
        })
    })
    .unwrap_or_else(|| UNTITLED.to_owned())
}
