use super::*;
use sigil::prelude::{IntoNode, button, hairline, kv_row, spacer, status_bar};

pub(in crate::tui::sigil_view) fn status_strip(app: App) -> NodeBuilder {
    let (snapshot, plan_mode, context, tokens, cached) = {
        let model = app.model.get();
        let cached = (model.cached_tokens > 0 && model.prompt_tokens > 0)
            .then(|| model.cached_tokens.saturating_mul(100) / model.prompt_tokens);
        (
            model.snapshot.clone(),
            model.prefs.plan_mode,
            context(model.usage.last(), model.snapshot.context_window, false),
            model.prompt_tokens.saturating_add(model.completion_tokens),
            cached,
        )
    };
    let branch = app.git_branch.clone().unwrap_or_default();
    let mut modes = container()
        .layout(
            LayoutStyle::flex_row()
                .height(26.0)
                .padding(2.0)
                .gap(2.0)
                .align_items(AlignItems::Center),
        )
        .class(classes!("rounded-md bg-gray-3"));
    let auto = app.clone();
    modes = modes.child(seg(
        crate::RunMode::Auto.name(),
        !snapshot.plan,
        move || {
            auto.leave_plan();
        },
    ));
    if plan_mode || snapshot.plan {
        let target = app.clone();
        modes = modes.child(seg(crate::RunMode::Plan.name(), snapshot.plan, move || {
            target.enter_plan()
        }));
    }
    let mut facts = container()
        .layout(
            LayoutStyle::flex_row()
                .gap(8.0)
                .align_items(AlignItems::Center),
        )
        .child(modes)
        .child(hairline())
        .child(kv_row(
            "context",
            label(context).class(classes!("text-xs font-mono text-gray-12")),
        ))
        .child(hairline())
        .child(kv_row(
            "session",
            label(format!("{} tokens", crate::provider::compact(tokens)))
                .class(classes!("text-xs font-mono text-gray-12")),
        ));
    if let Some(cached) = cached {
        facts =
            facts.child(label(format!("{cached}% cached")).class(classes!("text-xs text-gray-11")));
    }
    let git = container()
        .layout(
            LayoutStyle::flex_row()
                .gap(6.0)
                .align_items(AlignItems::Center),
        )
        .child(icons::branch())
        .child(label(branch).class(classes!("text-xs text-gray-11")));
    status_bar([facts, spacer(), git])
}

pub(super) fn seg(title: &'static str, on: bool, act: impl Fn() + 'static) -> impl IntoNode {
    let mut control = button(title).ghost().activate(act);
    if on {
        control = control.class("bg-gray-1");
    }
    control
}
