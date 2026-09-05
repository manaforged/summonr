use crate::prefs::Prefs;
use crate::tui::{Info, Round, Snapshot, fmt_tokens, short_session};

use super::super::model::Model;

pub(super) fn header(model: &Model, info: &Info) -> String {
    let relay = model
        .relay
        .as_deref()
        .map_or_else(String::new, |relay| format!(" · {relay}"));
    let mode = if model.snapshot.plan { "plan" } else { "auto" };
    format!(
        "summoner  {}{}   session {}   {mode}",
        model.model,
        relay,
        short_session(&info.session),
    )
}

pub(super) fn status(model: &Model, info: &Info) -> String {
    let total = model.prompt_tokens + model.completion_tokens;
    let activity = model.busy_since.map_or_else(
        || "ready".to_string(),
        |start| {
            let tool = model
                .live_tool
                .as_ref()
                .map_or_else(String::new, |(name, arg)| format!(" · {name} {arg}"));
            format!(
                "working · r{} · {}s{tool}",
                model.rounds.max(1),
                start.elapsed().as_secs()
            )
        },
    );
    let cost = if model.usage.iter().any(|round| round.cost.is_some()) {
        format!(
            " · {}",
            crate::dollars(model.usage.iter().filter_map(|round| round.cost).sum())
        )
    } else {
        String::new()
    };
    format!(
        "{} · {} tokens{cost} · {}% cached · {} peer{} · {activity}",
        info.workspace,
        fmt_tokens(total),
        cache_rate(model),
        model.peers,
        if model.peers == 1 { "" } else { "s" },
    )
}

fn cache_rate(model: &Model) -> u64 {
    model
        .cached_tokens
        .saturating_mul(100)
        .checked_div(model.prompt_tokens)
        .unwrap_or_default()
}

pub(super) fn settings_text(snapshot: &Snapshot, prefs: &Prefs) -> String {
    format!(
        "mode      {}\nverify    {}\nrounds    {}\nendpoint  {}\nmodel     {}\ntheme     {}\nreasoning {}\ntools     {}\nbus       {}",
        if snapshot.plan { "plan" } else { "auto" },
        snapshot.verify.as_deref().unwrap_or("off"),
        snapshot.rounds_label(),
        snapshot.endpoint,
        snapshot.model,
        prefs.theme,
        if prefs.reasoning { "on" } else { "off" },
        prefs.tools.name(),
        if prefs.bus { "on" } else { "off" },
    )
}

pub(super) fn facts(model: &Model, info: &Info) -> String {
    let relay = model
        .relay
        .as_deref()
        .map_or_else(String::new, |relay| format!(" · {relay}"));
    let mut rows = vec![
        format!("summoner  {}", info.version),
        format!("   model  {}{relay}", model.model),
        format!(" session  {}", short_session(&info.session)),
        format!(" workdir  {}", info.workspace),
    ];
    if model.snapshot.plan {
        rows.push("    mode  plan · read-only".to_string());
    }
    if model.snapshot.strict {
        rows.push("  policy  strict".to_string());
    }
    rows.join("\n")
}

pub(in crate::tui::sigil_view) fn stats(rounds: &[Round]) -> String {
    if rounds.is_empty() {
        return "No token usage yet.".to_string();
    }
    let priced = rounds.iter().any(|round| round.cost.is_some());
    let mut rows = vec![format!(
        "round | prompt | completion | reasoning | cached | prefix{}",
        if priced { " | cost" } else { "" }
    )];
    for (index, round) in rounds.iter().enumerate() {
        let mut row = format!(
            "{} | {} | {} | {} | {} · total {} | {}",
            index + 1,
            round.prompt,
            round.completion,
            round.reasoning,
            round.cached,
            round.total(),
            round.prefix_hash,
        );
        if priced {
            row.push_str(&format!(
                " | {}",
                round.cost.map_or_else(|| "—".to_string(), crate::dollars)
            ));
        }
        rows.push(row);
    }
    if priced {
        rows.push(format!(
            "total {}",
            crate::dollars(rounds.iter().filter_map(|round| round.cost).sum())
        ));
    }
    rows.join("\n")
}
