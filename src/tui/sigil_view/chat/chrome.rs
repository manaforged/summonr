use crate::tui::short_session;
use std::path::Path;

use sigil::prelude::Rgba8;

use super::super::app::App;
use super::super::canvas::palette;
use super::super::canvas::{Span, truncate};
use super::super::model::Model;
use super::super::speed::Speed;

pub(super) fn header(model: &Model, fps: u32, width: usize, accent: Rgba8) -> Vec<Span> {
    let mut left = vec![
        Span::styled("summonr ", accent).bold(),
        Span::text(model.snapshot.model.clone()),
    ];
    if let Some(effort) = model
        .snapshot
        .effort
        .filter(|effort| *effort != crate::provider::Effort::None)
    {
        left.push(Span::styled(format!("({})", effort.as_str()), palette().yellow).bold());
    }
    if width >= 80
        && let Some(relay) = model
            .snapshot
            .relay
            .as_deref()
            .filter(|relay| !relay.is_empty())
    {
        left.push(Span::styled(format!(" · {relay}"), palette().dim));
    }
    let (mode, mode_color) = if model.snapshot.plan {
        ("plan", palette().cyan)
    } else if model.snapshot.work {
        ("work", palette().yellow)
    } else if model.snapshot.goal {
        ("goal", palette().red)
    } else {
        ("auto", palette().green)
    };
    let mut right = Vec::new();
    if width >= 70 {
        right.push(Span::styled(
            format!("ses {} ", short_session(&model.session)),
            palette().dim,
        ));
    }
    right.push(Span::styled(format!("{mode} "), mode_color));
    if width >= 70 && model.snapshot.strict {
        right.push(Span::styled("strict ", palette().yellow));
    }
    if width >= 48 && fps > 0 {
        right.push(Span::styled(format!("{fps} fps "), palette().dim));
    }
    join(left, right, width)
}

pub(super) fn status_left(model: &Model, app: &App, width: usize, accent: Rgba8) -> Vec<Span> {
    let mut left = vec![
        Span::styled("📂 ", palette().gray),
        Span::styled(&app.info.workspace, palette().text),
    ];
    if let Some(branch) = &app.git_branch {
        left.push(Span::styled("  ⎇ ", palette().gray));
        left.push(Span::styled(branch, palette().yellow));
    }
    let live = model.agents.iter().filter(|agent| !agent.done).count();
    if live > 0 {
        left.push(Span::styled(
            format!("  · {live} agent{} live", if live == 1 { "" } else { "s" }),
            accent,
        ));
    }
    if line_width(&left) >= width {
        let name = Path::new(&app.info.workspace)
            .file_name()
            .unwrap_or_else(|| app.info.workspace.as_ref());
        left[1].text = name.to_string_lossy().into_owned();
    }
    if line_width(&left) >= width {
        left.truncate(2);
    }
    if line_width(&left) >= width {
        left.remove(0);
    }
    clip_line(left, width)
}

pub(super) fn idle_speed(model: &Model) -> Option<Span> {
    if model.busy_since.is_some() {
        return None;
    }
    model
        .speed
        .last()
        .map(|rate| Span::styled(format!(" · {}", Speed::label(rate)), palette().dim))
}

pub(super) fn context_meter(model: &Model, width: usize) -> Vec<Span> {
    let (_, _, percent) = fill(model);
    let bar = if width >= 22 { 8 } else { 0 };
    let mut parts = vec![Span::styled("ctx ", palette().dim)];
    if bar > 0 {
        let (_, window, _) = fill(model);
        let filled = if window > 0 {
            (percent as usize * bar) / 100
        } else {
            0
        };
        parts.push(Span::styled("[", palette().gray));
        parts.push(Span::styled("█".repeat(filled), palette().accent));
        parts.push(Span::styled(
            "░".repeat(bar.saturating_sub(filled)),
            palette().gray,
        ));
        parts.push(Span::styled("] ", palette().gray));
    }
    parts.push(Span::styled(format!("{percent}%"), palette().text));
    if line_width(&parts) > width {
        vec![Span::styled(format!("{percent}%"), palette().text)]
    } else {
        parts
    }
}

pub(super) fn cache_meter(model: &Model, width: usize) -> Vec<Span> {
    let hit = cache_hit(model);
    let mut parts = vec![Span::styled("cache ", palette().dim)];
    let spark = sparkline(model);
    if !spark.is_empty() && width >= 22 {
        parts.push(Span::styled(spark, palette().cyan));
        parts.push(Span::text(" "));
    }
    parts.push(Span::styled(format!("{hit}%"), palette().cyan));
    if line_width(&parts) > width {
        vec![Span::styled(format!("{hit}%"), palette().cyan)]
    } else {
        parts
    }
}

fn sparkline(model: &Model) -> String {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let rounds = model.usage.iter().rev().take(8).collect::<Vec<_>>();
    if rounds.len() < 2 {
        return String::new();
    }
    rounds
        .into_iter()
        .rev()
        .map(|round| {
            let hit = if round.prompt == 0 {
                0
            } else {
                u64::from(round.cached.min(round.prompt)) * 7 / u64::from(round.prompt.max(1))
            };
            BLOCKS[hit as usize]
        })
        .collect()
}

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

pub(super) fn context_card(model: &Model) -> Vec<Vec<Span>> {
    let (used, window, ctx) = fill(model);
    let mut lines = vec![vec![Span::styled(format!("{ctx}%"), palette().text)]];
    if window > 0 {
        lines.push(vec![Span::styled(
            format!(
                "{} / {}",
                crate::provider::compact(u64::from(used)),
                crate::provider::compact(u64::from(window))
            ),
            palette().dim,
        )]);
    }
    let out = model
        .completion_tokens
        .saturating_sub(model.reasoning_tokens);
    if model.reasoning_tokens > 0 {
        lines.push(vec![Span::styled(
            format!("think {}", crate::provider::compact(model.reasoning_tokens)),
            palette().gray,
        )]);
    }
    if out > 0 {
        lines.push(vec![Span::styled(
            format!("out {}", crate::provider::compact(out)),
            palette().gray,
        )]);
    }
    lines
}

pub(super) fn cache_card(model: &Model) -> Vec<Vec<Span>> {
    let hit = cache_hit(model);
    let mut lines = vec![vec![Span::styled(format!("{hit}%"), palette().cyan)]];
    if let Some(last) = last_hit(model) {
        lines.push(vec![Span::styled(format!("last {last}%"), palette().dim)]);
    }
    if model.cached_tokens > 0 {
        lines.push(vec![Span::styled(
            format!("cached {}", crate::provider::compact(model.cached_tokens)),
            palette().gray,
        )]);
    }
    if model.prompt_tokens > 0 {
        lines.push(vec![Span::styled(
            format!("in {}", crate::provider::compact(model.prompt_tokens)),
            palette().gray,
        )]);
        let fresh = model.prompt_tokens.saturating_sub(model.cached_tokens);
        lines.push(vec![Span::styled(
            format!("fresh {}", crate::provider::compact(fresh)),
            palette().gray,
        )]);
    }
    lines
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

fn join(mut left: Vec<Span>, right: Vec<Span>, width: usize) -> Vec<Span> {
    let right = clip_line(right, width);
    let right_width = line_width(&right);
    let gap = usize::from(right_width < width);
    left = clip_line(left, width.saturating_sub(right_width + gap));
    let used = line_width(&left) + right_width;
    left.push(Span::text(" ".repeat(width.saturating_sub(used))));
    left.extend(right);
    left
}

fn clip_line(mut line: Vec<Span>, width: usize) -> Vec<Span> {
    let mut used = 0;
    for span in &mut line {
        truncate(&mut span.text, width.saturating_sub(used));
        used += span.width();
    }
    line.retain(|span| !span.text.is_empty());
    line
}

pub(super) fn line_width(line: &[Span]) -> usize {
    line.iter().map(Span::width).sum()
}
