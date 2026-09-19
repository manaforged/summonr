use super::*;

pub(super) fn status_strip(app: App) -> NodeBuilder {
    let (plan, context, tokens, cached) = {
        let model = app.model.borrow();
        let cached = (model.cached_tokens > 0 && model.prompt_tokens > 0)
            .then(|| model.cached_tokens.saturating_mul(100) / model.prompt_tokens);
        (
            model.snapshot.plan,
            context(model.usage.last(), model.snapshot.context_window, false),
            model.prompt_tokens.saturating_add(model.completion_tokens),
            cached,
        )
    };
    let branch = app.git_branch.clone().unwrap_or_default();
    let auto = app.clone();
    let planned = app.clone();
    let mut strip = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(32.0)
                .padding_x(24.0)
                .align_items(AlignItems::Center)
                .gap(8.0)
                .shrink(0.0),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .height(26.0)
                        .padding(2.0)
                        .gap(2.0)
                        .align_items(AlignItems::Center),
                )
                .class(classes!("rounded-md bg-gray-3"))
                .child(seg("Auto", !plan, move || {
                    if auto.model.borrow().snapshot.plan {
                        auto.toggle_mode();
                    }
                }))
                .child(seg("Plan", plan, move || {
                    if !planned.model.borrow().snapshot.plan {
                        planned.toggle_mode();
                    }
                })),
        )
        .child(hairline())
        .child(metric("context", context))
        .child(hairline())
        .child(metric(
            "session",
            format!("{} tokens", crate::provider::compact(tokens)),
        ));
    if let Some(cached) = cached {
        strip =
            strip.child(label(format!("{cached}% cached")).class(classes!("text-xs text-gray-11")));
    }
    strip
        .child(spacer())
        .child(icons::branch())
        .child(label(branch).class(classes!("text-xs text-gray-11")))
}

pub(super) fn metric(name: &'static str, value: String) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .align_items(AlignItems::Center)
                .gap(4.0),
        )
        .child(label(name).class(classes!("text-xs text-gray-11")))
        .child(label(value).class(classes!("text-xs font-mono text-gray-12")))
}

pub(super) fn seg(title: &'static str, on: bool, act: impl Fn() + 'static) -> NodeBuilder {
    hit(title.to_string(), false, act)
        .layout(
            LayoutStyle::flex_row()
                .height(22.0)
                .padding_x(10.0)
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::Center),
        )
        .class(classes!("rounded-sm"))
        .class_when(signal(on), "bg-gray-1")
        .child(
            label(title)
                .class(classes!("text-xs text-gray-11"))
                .class_when(signal(on), "text-gray-12"),
        )
}
