use std::collections::BTreeMap;

use super::fmt::*;
use super::health::Health;
use super::{Call, Round, TOP};

pub(super) struct View<'a> {
    pub(super) model: &'a str,
    pub(super) turns: usize,
    pub(super) rounds: &'a [Round],
    pub(super) calls: &'a [Call],
    pub(super) harness: u64,
    pub(super) extra: (u64, u64, u64, u64),
    pub(super) prefix: u64,
    pub(super) history: (u64, u64, u64, u64),
    pub(super) footprint_kb: Option<u64>,
}

pub(super) fn render(view: &View) -> String {
    let View {
        model,
        turns,
        rounds,
        calls,
        harness,
        extra,
        prefix,
        history,
        footprint_kb,
    } = *view;
    let model_ms = rounds.iter().map(|round| round.millis).sum::<u64>() + extra.0;
    let tool_ms = calls.iter().map(|call| call.millis).sum::<u64>();
    let total = model_ms + tool_ms + harness;
    let input = rounds.iter().map(|round| round.input).sum::<u64>() + extra.1;
    let cached = rounds.iter().map(|round| round.cached).sum::<u64>() + extra.2;
    let output = rounds.iter().map(|round| round.output).sum::<u64>() + extra.3;
    let mut out = String::new();
    line(
        &mut out,
        format!(
            "{model} · {turns} turn(s) · {} model round(s) · {} tool call(s) · {}{}",
            rounds.len(),
            calls.len(),
            secs(total),
            footprint_kb
                .map(|kb| format!(" · mem {}", crate::text::megabytes(kb)))
                .unwrap_or_default()
        ),
    );
    line(
        &mut out,
        format!(
            "time    model {} ({}) · tools {} ({}) · harness {} ({})",
            secs(model_ms),
            share(model_ms, total),
            secs(tool_ms),
            share(tool_ms, total),
            secs(harness),
            share(harness, total)
        ),
    );
    line(
        &mut out,
        format!(
            "tokens  in {} ({} cached) · out {} · prompt {} → {}",
            count(input),
            share(cached, input),
            count(output),
            count(rounds.first().map_or(0, |round| round.input)),
            count(rounds.last().map_or(0, |round| round.input))
        ),
    );
    if !rounds.is_empty() {
        let warm_input: u64 = rounds.iter().skip(1).map(|round| round.input).sum();
        let warm_cached: u64 = rounds.iter().skip(1).map(|round| round.cached).sum();
        let uncached = input.saturating_sub(cached);
        let avg_request: u64 =
            rounds.iter().map(|round| round.request_bytes).sum::<u64>() / rounds.len() as u64;
        let final_history = history.0 + history.1 + history.2 + history.3;
        line(
            &mut out,
            format!(
                "economy  prefix {} B · avg request {} B · context now {} B (results {})",
                count(prefix),
                count(avg_request),
                count(final_history),
                share(history.2, final_history.max(1))
            ),
        );
        line(
            &mut out,
            format!(
                "cache    cold {} hit · warm {} hit · uncached {} in ({} per round)",
                share(rounds[0].cached, rounds[0].input),
                share(warm_cached, warm_input),
                count(uncached),
                count(uncached / rounds.len() as u64)
            ),
        );
    }
    if extra.0 > 0 {
        line(
            &mut out,
            format!(
                "        compaction and side calls {} · in {} · out {}",
                secs(extra.0),
                count(extra.1),
                count(extra.3)
            ),
        );
    }
    let failed = calls.iter().filter(|call| !call.success).count();
    if failed > 0 {
        line(&mut out, format!("failed  {failed} tool call(s)"));
    }

    let mut tally = Health::default();
    let mut first_edit = None;
    for (index, call) in calls.iter().enumerate() {
        if tally.add(&call.name, &call.arguments, call.success) && first_edit.is_none() {
            first_edit = Some(index + 1);
        }
    }
    if !calls.is_empty() {
        let mut health = tally.lines();
        health.push(match first_edit {
            Some(index) => format!("first successful edit/write at call {index}"),
            None => "no successful edit/write".to_string(),
        });
        section(&mut out, "tool health", health.into_iter());
    }

    let mut slow: Vec<&Call> = calls.iter().collect();
    slow.sort_by_key(|call| std::cmp::Reverse(call.millis));
    section(
        &mut out,
        "slowest tools",
        slow.iter().take(TOP).map(|call| {
            format!(
                "{:>8}  {:<12} {}",
                secs(call.millis),
                call.name,
                brief(&call.arguments)
            )
        }),
    );

    let mut waits: Vec<&Round> = rounds.iter().collect();
    waits.sort_by_key(|round| std::cmp::Reverse(round.millis));
    section(
        &mut out,
        "slowest model rounds",
        waits.iter().take(TOP).map(|round| {
            format!(
                "{:>8}  round {:<3} in {} ({} cached) · out {} · {} call(s)",
                secs(round.millis),
                round.index,
                count(round.input),
                share(round.cached, round.input),
                count(round.output),
                round.calls
            )
        }),
    );

    let mut large: Vec<&Call> = calls.iter().collect();
    large.sort_by_key(|call| std::cmp::Reverse(call.chars));
    section(
        &mut out,
        "largest results",
        large.iter().take(TOP).map(|call| {
            format!(
                "{:>8}  {:<12} {}",
                count(call.chars as u64),
                call.name,
                brief(&call.arguments)
            )
        }),
    );

    let mut seen: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for call in calls {
        *seen.entry((&call.name, &call.arguments)).or_default() += 1;
    }
    let mut repeats: Vec<_> = seen.into_iter().filter(|(_, times)| *times > 1).collect();
    repeats.sort_by_key(|(_, times)| std::cmp::Reverse(*times));
    section(
        &mut out,
        "repeated calls",
        repeats.iter().take(TOP).map(|((name, arguments), times)| {
            format!("{:>7}×  {:<12} {}", times, name, brief(arguments))
        }),
    );
    out
}
