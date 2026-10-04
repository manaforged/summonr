use super::Model;

fn fill(model: &Model) -> (u32, u32, u64) {
    let window = model.snapshot.context_window.unwrap_or(0);
    let used = model
        .usage
        .last()
        .map(|round| round.prompt.saturating_add(round.completion))
        .unwrap_or(0);
    let percent = if window == 0 {
        0
    } else {
        u64::from(used.min(window)) * 100 / u64::from(window)
    };
    (used, window, percent)
}

pub(in crate::tui::sigil_view) fn context_percent(model: &Model) -> u64 {
    fill(model).2
}

pub(in crate::tui::sigil_view) fn cache_percent(model: &Model) -> u64 {
    cache_hit(model)
}

pub(super) fn context_facts(model: &Model) -> Vec<(String, String)> {
    let (used, window, ctx) = fill(model);
    let mut rows = vec![("used".to_string(), format!("{ctx}%"))];
    if window > 0 {
        rows.push((
            "window".to_string(),
            format!(
                "{} / {}",
                crate::provider::compact(u64::from(used)),
                crate::provider::compact(u64::from(window))
            ),
        ));
    }
    let out = model
        .completion_tokens
        .saturating_sub(model.reasoning_tokens);
    if model.reasoning_tokens > 0 {
        rows.push((
            "think".to_string(),
            crate::provider::compact(model.reasoning_tokens),
        ));
    }
    if out > 0 {
        rows.push(("out".to_string(), crate::provider::compact(out)));
    }
    rows
}

pub(super) fn cache_facts(model: &Model) -> Vec<(String, String)> {
    let mut rows = vec![("hit".to_string(), format!("{}%", cache_hit(model)))];
    if let Some(last) = last_hit(model) {
        rows.push(("last".to_string(), format!("{last}%")));
    }
    if model.cached_tokens > 0 {
        rows.push((
            "cached".to_string(),
            crate::provider::compact(model.cached_tokens),
        ));
    }
    if model.prompt_tokens > 0 {
        rows.push((
            "in".to_string(),
            crate::provider::compact(model.prompt_tokens),
        ));
        let fresh = model.prompt_tokens.saturating_sub(model.cached_tokens);
        rows.push(("fresh".to_string(), crate::provider::compact(fresh)));
    }
    rows
}

fn cache_hit(model: &Model) -> u64 {
    if model.prompt_tokens == 0 {
        return 0;
    }
    model.cached_tokens.min(model.prompt_tokens) * 100 / model.prompt_tokens.max(1)
}

fn last_hit(model: &Model) -> Option<u64> {
    let round = model.usage.last()?;
    if round.prompt == 0 {
        return None;
    }
    Some(u64::from(round.cached.min(round.prompt)) * 100 / u64::from(round.prompt.max(1)))
}
