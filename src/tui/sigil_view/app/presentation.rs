use crate::prefs::Prefs;
use crate::tui::{Activity, Info, Round, Snapshot};

use super::model::Model;

impl Model {
    pub(super) fn header(&self) -> String {
        let relay = self
            .snapshot
            .relay
            .as_deref()
            .map_or_else(String::new, |relay| format!(" · {relay}"));
        let mode = self.snapshot.mode_name();
        format!(
            "summonr  {}{}   session {}   {mode}",
            self.snapshot.model,
            relay,
            crate::session::short(&self.session),
        )
    }

    pub(super) fn status_line(&self, info: &Info) -> String {
        let total = self.prompt_tokens + self.completion_tokens;
        let activity = self.busy_since.map_or_else(
            || "ready".to_string(),
            |(activity, start)| {
                if activity != Activity::Turn {
                    return format!("{} · {}s", activity.label(), start.elapsed().as_secs());
                }
                let tool = self
                    .live_tool
                    .as_ref()
                    .map_or_else(String::new, |(name, arg)| format!(" · {name} {arg}"));
                format!("working · {}s{tool}", start.elapsed().as_secs())
            },
        );
        let cost = self
            .spend()
            .map_or_else(String::new, |spend| format!(" · {}", crate::dollars(spend)));
        format!(
            "{} · {} tokens{cost} · {}% cached · {} peer{} · {activity}",
            info.workspace,
            crate::provider::compact(total),
            self.cache_rate(),
            self.peers.len(),
            if self.peers.len() == 1 { "" } else { "s" },
        )
    }

    fn cache_rate(&self) -> u64 {
        self.cached_tokens
            .saturating_mul(100)
            .checked_div(self.prompt_tokens)
            .unwrap_or_default()
    }

    pub(super) fn facts(&self, info: &Info) -> String {
        let relay = self
            .snapshot
            .relay
            .as_deref()
            .map_or_else(String::new, |relay| format!(" · {relay}"));
        let mut rows = vec![
            format!("summonr  {}", info.version),
            format!("   model  {}{relay}", self.snapshot.model),
            format!(" session  {}", crate::session::short(&self.session)),
            format!(" workdir  {}", info.workspace),
        ];
        if self.snapshot.strict {
            rows.push("  policy  strict".to_string());
        }
        rows.join("\n")
    }
}

pub(super) fn settings_text(snapshot: &Snapshot, prefs: &Prefs) -> String {
    format!(
        "mode      {}\nverify    {}\nendpoint  {}\nmodel     {}\ntheme     {}\nreasoning {}\ntools     {}\nmail      {}",
        snapshot.mode_name(),
        snapshot.verify.as_deref().unwrap_or("off"),
        snapshot.endpoint,
        snapshot.model,
        prefs.theme,
        prefs.reasoning.name(),
        prefs.tools.name(),
        if prefs.mail { "on" } else { "off" },
    )
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
        if self.model.get().prefs.diff_layout == layout {
            return;
        }
        self.flip(|prefs| prefs.diff_layout = layout);
    }
}
