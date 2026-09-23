use std::collections::BTreeMap;
use std::path::Path;

use crate::durable::log::{self, Line};
use crate::error::Result;

const UNACCOUNTED: &str = "unaccounted turn time";
const TOP: usize = 5;

struct Call {
    name: String,
    arguments: String,
    millis: u64,
    chars: usize,
    success: bool,
}

struct Round {
    index: usize,
    millis: u64,
    input: u64,
    cached: u64,
    output: u64,
    calls: usize,
    request_bytes: u64,
}

pub fn profile(path: &Path) -> Result<String> {
    let mut model = String::new();
    let mut rounds: Vec<Round> = Vec::new();
    let mut pending: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut calls: Vec<Call> = Vec::new();
    let mut harness = 0_u64;
    let mut extra = (0_u64, 0_u64, 0_u64, 0_u64);
    let mut turns = 0_usize;
    let mut prefix = 0_u64;
    let mut history = (0_u64, 0_u64, 0_u64, 0_u64);
    let mut first_user = 0_u64;
    let mut rss_kb = None;
    for line in log::read(path)? {
        match line {
            Line::Session {
                model: name,
                system,
                ..
            } => {
                model = name;
                prefix = system.len() as u64;
            }
            Line::Tools { bytes } => prefix = prefix.saturating_add(bytes),
            Line::User {
                start: true, text, ..
            } => {
                turns += 1;
                history.0 = history.0.saturating_add(text.len() as u64);
                if first_user == 0 {
                    first_user = text.len() as u64;
                }
            }
            Line::User { text, .. } => {
                history.0 = history.0.saturating_add(text.len() as u64);
            }
            Line::Assistant {
                tool_calls,
                usage,
                text,
                reasoning,
                ..
            } => {
                let request_bytes = prefix
                    .saturating_add(history.0)
                    .saturating_add(history.1)
                    .saturating_add(history.2)
                    .saturating_add(history.3);
                history.1 = history.1.saturating_add(
                    (text.len() + reasoning.len()) as u64
                        + tool_calls
                            .iter()
                            .map(|call| call.function.arguments.len() as u64 + 40)
                            .sum::<u64>(),
                );
                for call in &tool_calls {
                    pending.insert(
                        call.id.clone(),
                        (call.function.name.clone(), call.function.arguments.clone()),
                    );
                }
                rounds.push(Round {
                    index: rounds.len() + 1,
                    millis: usage.millis,
                    input: usage.input,
                    cached: usage.cached,
                    output: usage.output,
                    calls: tool_calls.len(),
                    request_bytes,
                });
            }
            Line::Tool {
                call,
                text,
                success,
                millis,
            } => {
                history.2 = history.2.saturating_add(text.len() as u64 + 40);
                let (name, arguments) = pending.remove(&call).unwrap_or_default();
                calls.push(Call {
                    name,
                    arguments,
                    millis,
                    chars: text.chars().count(),
                    success,
                });
            }
            Line::Charge { usage, note, .. } if note == UNACCOUNTED => {
                harness = harness.saturating_add(usage.millis);
            }
            Line::Charge { usage, .. } => {
                extra.0 = extra.0.saturating_add(usage.millis);
                extra.1 = extra.1.saturating_add(usage.input);
                extra.2 = extra.2.saturating_add(usage.cached);
                extra.3 = extra.3.saturating_add(usage.output);
            }
            Line::Receipt { text } => {
                history.3 = history.3.saturating_add(text.len() as u64);
            }
            Line::Summary { text, tail } => {
                history.0 = first_user;
                history.1 = 0;
                history.2 = 0;
                history.3 = (text.len()
                    + tail
                        .iter()
                        .map(|message| message.content.len() + 40)
                        .sum::<usize>()) as u64;
            }
            Line::Clear => history = (0, 0, 0, 0),
            Line::Budget { .. }
            | Line::Workflow { .. }
            | Line::Child { .. }
            | Line::Rewrite { .. }
            | Line::Interrupted { .. } => {}
            Line::Close { rss_kb: kb, .. } => rss_kb = kb,
        }
    }
    let view = View {
        model: &model,
        turns,
        rounds: &rounds,
        calls: &calls,
        harness,
        extra,
        prefix,
        history,
        rss_kb,
    };
    Ok(render(&view))
}

struct View<'a> {
    model: &'a str,
    turns: usize,
    rounds: &'a [Round],
    calls: &'a [Call],
    harness: u64,
    extra: (u64, u64, u64, u64),
    prefix: u64,
    history: (u64, u64, u64, u64),
    rss_kb: Option<u64>,
}

fn render(view: &View) -> String {
    let View {
        model,
        turns,
        rounds,
        calls,
        harness,
        extra,
        prefix,
        history,
        rss_kb,
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
            rss_kb
                .map(|kb| format!(" · rss {:.0} MB", kb as f64 / 1024.0))
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

    let mut per_tool: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut native = 0_usize;
    let mut shell = 0_usize;
    let mut commands = 0_usize;
    let mut pipes = 0_usize;
    let mut first_edit = None;
    for (index, call) in calls.iter().enumerate() {
        let entry = per_tool.entry(call.name.as_str()).or_default();
        entry.0 += 1;
        if !call.success {
            entry.1 += 1;
        }
        let args: serde_json::Value = serde_json::from_str(&call.arguments).unwrap_or_default();
        if call.name == "run_command" {
            commands += 1;
            let command = args
                .get("command")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if crate::tools::command::searchish(command) {
                if crate::tools::command::native_read_args(command).is_some() {
                    native += 1;
                } else {
                    shell += 1;
                }
            }
            if head_tail(command) {
                pipes += 1;
            }
        } else if call.name == "read" {
            let op = args
                .get("op")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let pattern = args
                .get("pattern")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if op == "search" || (op.is_empty() && !pattern.is_empty()) {
                native += 1;
            }
        } else if call.name == "grep" {
            native += 1;
        }
        if first_edit.is_none() && call.success && matches!(call.name.as_str(), "edit" | "write") {
            first_edit = Some(index + 1);
        }
    }
    if !calls.is_empty() {
        let mut tools: Vec<String> = per_tool
            .iter()
            .map(|(name, (calls, failed))| {
                if *failed > 0 {
                    format!("{name} {calls} ({failed} failed)")
                } else {
                    format!("{name} {calls}")
                }
            })
            .collect();
        tools.sort();
        let mut health = vec![format!("calls    {}", tools.join(" · "))];
        if native + shell > 0 {
            health.push(format!("search   native {native} · shell {shell}"));
        }
        if commands > 0 {
            health.push(format!(
                "pipes    {pipes} of {commands} run_command calls pipe to head/tail ({})",
                share(pipes as u64, commands as u64)
            ));
        }
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

fn line(out: &mut String, text: String) {
    out.push_str(&text);
    out.push('\n');
}

fn section(out: &mut String, title: &str, lines: impl Iterator<Item = String>) {
    let lines: Vec<String> = lines.collect();
    if lines.is_empty() {
        return;
    }
    line(out, format!("\n{title}"));
    for text in lines {
        line(out, text);
    }
}

mod aggregate;

pub fn profile_all(last: Option<usize>) -> Result<String> {
    aggregate::profile_all(last)
}

fn head_tail(command: &str) -> bool {
    command
        .split(['|', ';', '&'])
        .skip(1)
        .filter_map(|part| part.split_whitespace().next())
        .map(|verb| verb.rsplit('/').next().unwrap_or(verb))
        .any(|verb| matches!(verb, "head" | "tail" | "less" | "more"))
}

fn brief(arguments: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
    let text = value
        .get("command")
        .or_else(|| value.get("path"))
        .or_else(|| value.get("query"))
        .or_else(|| value.get("url"))
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| arguments.to_string(), str::to_string);
    crate::child::bounded(&text.split_whitespace().collect::<Vec<_>>().join(" "), 80)
}

fn secs(millis: u64) -> String {
    format!("{:.1}s", millis as f64 / 1000.0)
}

fn share(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "–".to_string();
    }
    format!("{:.0}%", part as f64 * 100.0 / whole as f64)
}

fn count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}
