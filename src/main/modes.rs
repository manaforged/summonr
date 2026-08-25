use std::io::Write;
use std::path::Path;

use summoner::{Client, Event, Loop, Result, Turn, UserInput};
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, BufReader};

use super::cli::Args;

pub(super) async fn print(mut loop_: Loop<Client>, args: &Args) -> Result<i32> {
    let prompt = if args.prompt.is_empty() {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input).await?;
        input
    } else {
        args.prompt.join(" ")
    };
    let input = summoner::attach(loop_.workspace(), &prompt)?;
    let turn = complete(&mut loop_, input, args.json).await?;
    if !args.json {
        println!();
    }
    Ok(exit(&turn))
}

pub(super) async fn steps(mut loop_: Loop<Client>, args: &Args, plan_path: &str) -> Result<i32> {
    let markdown = std::fs::read_to_string(plan_path)
        .map_err(|error| summoner::Error::Config(format!("cannot read {plan_path}: {error}")))?;
    let steps = summoner::steps::parse(&markdown)?;
    let total = steps.len();
    let checks = loop_.verify_commands().to_vec();
    loop_.select_steps(&markdown, &checks)?;
    loop {
        let progress = loop_.work_progress()?;
        if let Some(code) = progress.exit_code() {
            if !args.json {
                println!();
            }
            return Ok(code);
        }
        let current = progress
            .current
            .ok_or_else(|| summoner::Error::Config("Steps Work has no active item".to_string()))?;
        let step = steps.get(current.saturating_sub(1)).ok_or_else(|| {
            summoner::Error::Config("Steps Work is outside the selected plan".to_string())
        })?;
        step_event(args.json, current, total, "start", None);
        let turn = complete(
            &mut loop_,
            summoner::steps::prompt(step, total, plan_path).into(),
            args.json,
        )
        .await?;
        if turn.aborted {
            loop_.abandon_work("step execution aborted")?;
            return Ok(3);
        }
        let after = loop_.work_progress()?;
        if let Some(check) = turn.verify.as_ref() {
            step_event(
                args.json,
                current,
                total,
                if check.passed {
                    "check passed"
                } else {
                    "check failed"
                },
                Some(&check.command),
            );
        } else if after.current != Some(current) || after.terminal() {
            step_event(args.json, current, total, "done", None);
        }
    }
}

pub(super) async fn goal(
    mut loop_: Loop<Client>,
    args: &Args,
    objective: &str,
    gates: Vec<String>,
) -> Result<i32> {
    let max_turns = args
        .max_turns
        .map_or(summoner::DEFAULT_MAX_TURNS, |value| value as usize);
    loop_.select_goal(objective, &gates, max_turns)?;
    let code = drive_goal(&mut loop_, objective, &gates, args.json).await?;
    if !args.json {
        println!();
    }
    Ok(code)
}

/// Run turns until the durable Work verdict becomes terminal.
pub(super) async fn drive_goal(
    loop_: &mut Loop<Client>,
    objective: &str,
    gates: &[String],
    json: bool,
) -> Result<i32> {
    loop {
        let progress = loop_.work_progress()?;
        if let Some(code) = progress.exit_code() {
            return Ok(code);
        }
        let turn = complete(
            loop_,
            summoner::goal_prompt(objective, gates, progress.rounds).into(),
            json,
        )
        .await?;
        if turn.aborted {
            loop_.abandon_work("goal execution aborted")?;
            return Ok(3);
        }
    }
}

pub(super) async fn interactive(
    mut loop_: Loop<Client>,
    workspace: &Path,
    json: bool,
) -> Result<i32> {
    let mut lines = BufReader::new(io::stdin()).lines();
    loop {
        prompt()?;
        let Some(line) = lines.next_line().await? else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(keep_running) = command(&mut loop_, line).await {
            if keep_running {
                continue;
            }
            break;
        }
        let input = summoner::attach(workspace, line)?;
        drop(complete(&mut loop_, input, json).await?);
        if !json {
            println!();
        }
    }
    Ok(0)
}

async fn command(loop_: &mut Loop<Client>, line: &str) -> Option<bool> {
    match summoner::parse_command(line) {
        Some(summoner::SlashCommand::Exit) => Some(false),
        Some(summoner::SlashCommand::Settings) => {
            settings(loop_);
            Some(true)
        }
        Some(summoner::SlashCommand::Help) => {
            eprintln!("/exit  /session  /clear  /settings  /help");
            eprintln!("/plan  /plan accept  /plan reject  /auto");
            eprintln!("/goal <objective>  work toward it across turns (gates + judge)");
            eprintln!("@file prompt");
            Some(true)
        }
        Some(summoner::SlashCommand::PlanAccept) => Some(accept(loop_)),
        Some(summoner::SlashCommand::PlanReject) => Some(reject(loop_)),
        Some(summoner::SlashCommand::Plan) => Some(toggle_plan(loop_)),
        Some(summoner::SlashCommand::Auto) => Some(auto(loop_)),
        Some(summoner::SlashCommand::Goal) => Some(repl_goal(loop_, line).await),
        Some(summoner::SlashCommand::Session) => Some(session(loop_)),
        Some(summoner::SlashCommand::Clear) => Some(clear(loop_)),
        Some(_) => Some(unknown(line)),
        None if line.starts_with('/') => Some(unknown(line)),
        _ => None,
    }
}

async fn repl_goal(loop_: &mut Loop<Client>, line: &str) -> bool {
    let objective = line.trim_start_matches("/goal").trim();
    if objective.is_empty() {
        eprintln!("/goal <objective> — work toward it across turns");
        return true;
    }
    let gates = loop_.verify_commands().to_vec();
    if let Err(error) = loop_.select_goal(objective, &gates, summoner::DEFAULT_MAX_TURNS) {
        eprintln!("summon: {error}");
        return true;
    }
    let result = drive_goal(loop_, objective, &gates, false).await;
    match result {
        Ok(0) => eprintln!("\ngoal achieved"),
        Ok(code) => eprintln!("\ngoal stopped (exit {code}) — resume with `summon --goal`"),
        Err(error) => eprintln!("summon: {error}"),
    }
    true
}

fn settings(loop_: &Loop<Client>) {
    eprintln!("model      {}", loop_.model());
    eprintln!(
        "mode       {}",
        if loop_.is_plan() { "plan" } else { "auto" }
    );
    eprintln!(
        "verify     {}",
        verify_key(loop_).unwrap_or_else(|| "off".to_string())
    );
    eprintln!("max rounds {}", loop_.max_rounds());
    eprintln!("(the settings modal lives in the full-screen interface)");
}

/// The armed gate list as one identity string (job binding, display).
fn verify_key(loop_: &Loop<Client>) -> Option<String> {
    let gates = loop_.verify_commands();
    (!gates.is_empty()).then(|| gates.join("; "))
}

fn accept(loop_: &mut Loop<Client>) -> bool {
    if let Err(error) = loop_.accept_plan() {
        eprintln!("summon: {error}");
        return true;
    }
    loop_.set_plan(false);
    eprintln!("Plan accepted. Your next prompt executes in that Work.");
    true
}

fn reject(loop_: &mut Loop<Client>) -> bool {
    if let Err(error) = loop_.abandon_work("plan rejected by the user") {
        eprintln!("summon: {error}");
        return true;
    }
    loop_.set_plan(false);
    eprintln!("plan rejected");
    true
}

fn toggle_plan(loop_: &mut Loop<Client>) -> bool {
    let plan = !loop_.is_plan();
    loop_.set_plan(plan);
    eprintln!(
        "{}",
        if plan {
            "plan mode — read-only; the plan is published into the session journal"
        } else {
            "auto mode — full tools"
        }
    );
    true
}

fn auto(loop_: &mut Loop<Client>) -> bool {
    loop_.set_plan(false);
    eprintln!("auto mode — full tools");
    true
}

fn session(loop_: &Loop<Client>) -> bool {
    eprintln!("{}  {}", loop_.session_id(), loop_.session_path().display());
    true
}

fn clear(loop_: &mut Loop<Client>) -> bool {
    match loop_.clear_history() {
        Ok(()) => eprintln!("history cleared"),
        Err(error) => eprintln!("clear failed: {error}"),
    }
    true
}

fn unknown(line: &str) -> bool {
    let head = line.split_whitespace().next().unwrap_or(line);
    eprintln!("unknown command `{head}` — /help lists them");
    true
}

fn prompt() -> Result<()> {
    eprint!("> ");
    std::io::stderr()
        .flush()
        .map_err(|error| summoner::Error::Config(error.to_string()))
}

async fn complete(loop_: &mut Loop<Client>, input: UserInput, json: bool) -> Result<Turn> {
    if json {
        loop_.turn_with(input, print_json).await
    } else {
        loop_.turn_with(input, print_live).await
    }
}

fn print_json(event: &Event) {
    match serde_json::to_string(event) {
        Ok(line) => println!("{line}"),
        Err(error) => eprintln!("cannot encode event: {error}"),
    }
}

fn print_live(event: &Event) {
    match event {
        Event::ToolCall { name, .. } => eprintln!("\n→ {name}"),
        Event::ToolResult { success: false, .. } => eprintln!("  (failed)"),
        Event::Verify { passed: true, .. } => eprintln!("\n✓ verify passed"),
        Event::Verify { .. } => eprintln!("\n✗ verify failed"),
        Event::Work { progress } => eprintln!("\n{progress}"),
        Event::Receipt { text } => println!("\n{text}"),
        Event::Aborted { reason } => eprintln!("\n{reason}"),
        _ => {}
    }
}

fn exit(turn: &Turn) -> i32 {
    if turn.verify.as_ref().is_some_and(|verify| !verify.passed) {
        2
    } else if turn.aborted {
        3
    } else {
        0
    }
}

fn step_event(json: bool, index: usize, total: usize, state: &str, check: Option<&str>) {
    if json {
        println!(
            "{}",
            serde_json::json!({ "type": "step", "index": index, "total": total, "state": state, "check": check })
        );
    } else if let Some(check) = check {
        eprintln!("\n[step {index}/{total}] {state} · {check}");
    } else {
        eprintln!("\n[step {index}/{total}] {state}");
    }
}
