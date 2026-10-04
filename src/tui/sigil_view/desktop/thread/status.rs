use super::*;
use sigil::prelude::Paint;
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
        .rounded(sigil::prelude::Radius::Md)
        .bg(sigil::prelude::Token::Gray3);
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
            sigil::kernel::text(context)
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .font_family(sigil::prelude::FontFamily::Mono)
                .text_color(sigil::prelude::Token::Gray12),
        ))
        .child(hairline())
        .child(kv_row(
            "session",
            sigil::kernel::text(format!("{} tokens", crate::provider::compact(tokens)))
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .font_family(sigil::prelude::FontFamily::Mono)
                .text_color(sigil::prelude::Token::Gray12),
        ));
    if let Some(cached) = cached {
        facts = facts.child(
            sigil::kernel::text(format!("{cached}% cached"))
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        );
    }
    let git = container()
        .layout(
            LayoutStyle::flex_row()
                .gap(6.0)
                .align_items(AlignItems::Center),
        )
        .child(icons::branch())
        .child(
            sigil::kernel::text(branch)
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        );
    status_bar([facts, spacer(), git])
}

pub(super) fn seg(title: &'static str, on: bool, act: impl Fn() + 'static) -> impl IntoNode {
    let mut control = button(title).ghost().activate(act);
    if on {
        control = control.bg(sigil::prelude::Token::Gray1);
    }
    control
}
