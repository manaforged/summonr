use crate::prefs::Prefs;
use crate::tui::{Activity, Info, Round, Snapshot, short_session};

use super::super::model::Model;

pub(super) fn header(model: &Model) -> String {
    let relay = model
        .snapshot
        .relay
        .as_deref()
        .map_or_else(String::new, |relay| format!(" · {relay}"));
    let mode = if model.snapshot.plan { "plan" } else { "auto" };
    format!(
        "summonr  {}{}   session {}   {mode}",
        model.snapshot.model,
        relay,
        short_session(&model.session),
    )
}

pub(super) fn status(model: &Model, info: &Info) -> String {
    let total = model.prompt_tokens + model.completion_tokens;
    let activity = model.busy_since.map_or_else(
        || "ready".to_string(),
        |(activity, start)| {
            if activity != Activity::Turn {
                return format!("{} · {}s", activity.label(), start.elapsed().as_secs());
            }
            let tool = model
                .live_tool
                .as_ref()
                .map_or_else(String::new, |(name, arg)| format!(" · {name} {arg}"));
            format!("working · {}s{tool}", start.elapsed().as_secs())
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
        crate::provider::compact(total),
        cache_rate(model),
        model.peers.len(),
        if model.peers.len() == 1 { "" } else { "s" },
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
        "mode      {}\nverify    {}\nendpoint  {}\nmodel     {}\ntheme     {}\nreasoning {}\ntools     {}\nbus       {}",
        if snapshot.plan { "plan" } else { "auto" },
        snapshot.verify.as_deref().unwrap_or("off"),
        snapshot.endpoint,
        snapshot.model,
        prefs.theme,
        prefs.reasoning.name(),
        prefs.tools.name(),
        if prefs.bus { "on" } else { "off" },
    )
}

pub(super) fn facts(model: &Model, info: &Info) -> String {
    let relay = model
        .snapshot
        .relay
        .as_deref()
        .map_or_else(String::new, |relay| format!(" · {relay}"));
    let mut rows = vec![
        format!("summonr  {}", info.version),
        format!("   model  {}{relay}", model.snapshot.model),
        format!(" session  {}", short_session(&model.session)),
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
impl super::App {
    pub(in crate::tui::sigil_view) fn set_diff_layout(&self, layout: crate::prefs::DiffLayout) {
        if self.model.borrow().prefs.diff_layout == layout {
            return;
        }
        *self.history.borrow_mut() = super::History::default();
        self.flip(|prefs| prefs.diff_layout = layout);
    }
}
