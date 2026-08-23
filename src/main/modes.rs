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
    let contract = summoner::steps::contract(&markdown)?;
    let total = steps.len();
    let restored = loop_.restored_progress();
    let mut job = if let Some(state) = restored {
        summoner::steps::Job::from_state(loop_.workspace().to_path_buf(), state)
    } else {
        summoner::steps::Job::open(
            loop_.workspace(),
            std::path::Path::new(plan_path),
            loop_.session_id().as_str(),
            loop_.budget().max_tokens,
            loop_.budget().max_secs,
            verify_key(&loop_).as_deref(),
        )?
    };
    job.validate_steps(total)?;
    if job.state().last_completed_step.is_none() && job.state().terminal.is_none() {
        loop_.select_plan(&markdown)?;
    }
    loop_.persist_progress(job.state())?;
    if let Some(terminal) = &job.state().terminal {
        return Ok(terminal.exit_code());
    }
    let next = job.state().next_step;
    for (ordinal, step) in steps.iter().enumerate() {
        let ordinal = ordinal.saturating_add(1);
        if ordinal < next {
            continue;
        }
        if let Some(code) = run_step(&mut loop_, args, &mut job, step, total, plan_path).await? {
            let terminal = if code == 3 {
                summoner::steps::Terminal::Aborted {
                    reason: "step execution aborted".to_string(),
                }
            } else {
                summoner::steps::Terminal::Failed { code }
            };
            job.finish(terminal, loop_.spent_tokens(), loop_.elapsed_secs())?;
            loop_.persist_progress(job.state())?;
            return Ok(code);
        }
        job.complete_step(ordinal, loop_.spent_tokens(), loop_.elapsed_secs())?;
        loop_.persist_progress(job.state())?;
    }
    if let Some(code) = check_contract(&mut loop_, args, &mut job, &contract, total).await? {
        let terminal = if code == 3 {
            summoner::steps::Terminal::Aborted {
                reason: "step contract check aborted".to_string(),
            }
        } else {
            summoner::steps::Terminal::Failed { code }
        };
        job.finish(terminal, loop_.spent_tokens(), loop_.elapsed_secs())?;
        loop_.persist_progress(job.state())?;
        return Ok(code);
    }
    job.finish(
        summoner::steps::Terminal::Completed,
        loop_.spent_tokens(),
        loop_.elapsed_secs(),
    )?;
    loop_.persist_progress(job.state())?;
    if !args.json {
        println!();
    }
    Ok(0)
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
    let mut engine = if let Some(state) = loop_.restored_progress() {
        summoner::GoalEngine::from_state(loop_.workspace(), objective, gates, max_turns, state)?
    } else {
        summoner::GoalEngine::open(
            loop_.workspace(),
            objective,
            loop_.session_id().as_str(),
            gates,
            max_turns,
            loop_.budget(),
        )?
    };
    loop_.persist_progress(engine.snapshot())?;
    if engine.exit_code().is_some() {
        return Ok(engine.exit_code().unwrap_or(0));
    }
    let code = drive_goal(&mut loop_, &mut engine, args.json).await?;
    loop_.persist_progress(engine.snapshot())?;
    if !args.json {
        println!();
    }
    Ok(code)
}

/// The goal loop shared by `--goal` and `/goal`: the engine decides,
/// the caller executes turns and renders notes.
pub(super) async fn drive_goal(
    loop_: &mut Loop<Client>,
    engine: &mut summoner::GoalEngine,
    json: bool,
) -> Result<i32> {
    while engine.exit_code().is_none() {
        let Some(prompt) = engine.prompt()? else {
            break;
        };
        for note in engine.take_notes() {
            goal_note(json, &note);
        }
        let turn = complete(loop_, prompt.into(), json).await?;
        engine.after_turn(loop_, &turn).await?;
        for note in engine.take_notes() {
            goal_note(json, &note);
        }
    }
    for note in engine.take_notes() {
        goal_note(json, &note);
    }
    Ok(engine.exit_code().unwrap_or(0))
}

fn goal_note(json: bool, note: &str) {
    if json {
        println!("{}", serde_json::json!({ "type": "goal", "note": note }));
    } else {
        eprintln!("\n{note}");
    }
}

async fn run_step(
    loop_: &mut Loop<Client>,
    args: &Args,
    job: &mut summoner::steps::Job,
    step: &summoner::steps::Step,
    total: usize,
    path: &str,
) -> Result<Option<i32>> {
    step_event(args.json, step.index, total, "start", None);
    let prompt = summoner::steps::prompt(step, total, path);
    let turn = complete(loop_, prompt.into(), args.json).await?;
    job.progress(loop_.spent_tokens(), loop_.elapsed_secs())?;
    loop_.persist_progress(job.state())?;
    let code = exit(&turn);
    if code != 0 {
        step_event(
            args.json,
            step.index,
            total,
            if code == 3 { "aborted" } else { "failed" },
            None,
        );
        return Ok(Some(code));
    }
    if step.checks.is_empty() {
        step_event(args.json, step.index, total, "done", None);
        return Ok(None);
    }
    for check in &step.checks {
        if let Some(code) = check_step(loop_, args, job, step.index, total, check).await? {
            return Ok(Some(code));
        }
    }
    Ok(None)
}

async fn check_step(
    loop_: &mut Loop<Client>,
    args: &Args,
    job: &mut summoner::steps::Job,
    index: usize,
    total: usize,
    check: &str,
) -> Result<Option<i32>> {
    for attempt in 0..3 {
        let output = loop_
            .shell(check)
            .await
            .unwrap_or_else(|error| format!("exit 1\n{error}"));
        if summoner::steps::exit_code(&output) == Some(0) {
            step_event(args.json, index, total, "check passed", Some(check));
            return Ok(None);
        }
        step_event(args.json, index, total, "check failed", Some(check));
        if attempt == 2 {
            step_event(args.json, index, total, "gave up", Some(check));
            if !args.json {
                eprintln!("\n{output}");
            }
            return Ok(Some(2));
        }
        let prompt = summoner::steps::retry_prompt(check, &output);
        let turn = complete(loop_, prompt.into(), args.json).await?;
        job.progress(loop_.spent_tokens(), loop_.elapsed_secs())?;
        loop_.persist_progress(job.state())?;
        let code = exit(&turn);
        if code != 0 {
            return Ok(Some(code));
        }
    }
    Ok(None)
}

async fn check_contract(
    loop_: &mut Loop<Client>,
    args: &Args,
    job: &mut summoner::steps::Job,
    contract: &summoner::steps::Contract,
    total: usize,
) -> Result<Option<i32>> {
    if contract.items.is_empty() {
        return Ok(None);
    }
    for attempt in 0..3 {
        let mut missing = summoner::steps::missing(contract, loop_.workspace());
        for item in &contract.items {
            if let summoner::steps::Item::Stdout { command, expected } = item {
                let output = loop_
                    .shell(command)
                    .await
                    .unwrap_or_else(|error| format!("exit 1\n{error}"));
                if let Some(item) = summoner::steps::stdout_item(command, expected, &output) {
                    missing.push(item);
                }
            }
        }
        let passed = missing.is_empty();
        step_event(
            args.json,
            total,
            total,
            if passed {
                "contract met"
            } else {
                "contract not met"
            },
            Some(&format!("{} item(s)", contract.items.len())),
        );
        if passed {
            return Ok(None);
        }
        if attempt == 2 {
            if !args.json {
                eprintln!("\n{}", missing.join("\n"));
            }
            return Ok(Some(2));
        }
        let turn = complete(
            loop_,
            summoner::steps::contract_prompt(&missing).into(),
            args.json,
        )
        .await?;
        job.progress(loop_.spent_tokens(), loop_.elapsed_secs())?;
        loop_.persist_progress(job.state())?;
        let code = exit(&turn);
        if code != 0 {
            return Ok(Some(code));
        }
    }
    Ok(None)
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
        if let Some(keep_running) = command(&mut loop_, workspace, line).await {
            if keep_running {
                continue;
            }
            break;
        }
        if let Some(shell) = line.strip_prefix('!') {
            match loop_.shell(shell.trim()).await {
                Ok(output) => println!("{output}"),
                Err(error) => eprintln!("summon: {error}"),
            }
            continue;
        }
        let input = summoner::attach(workspace, line)?;
        drop(complete(&mut loop_, input, json).await?);
        if !json {
            println!();
        }
    }
    Ok(0)
}

async fn command(loop_: &mut Loop<Client>, workspace: &Path, line: &str) -> Option<bool> {
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
            eprintln!("!command   @file prompt");
            Some(true)
        }
        Some(summoner::SlashCommand::PlanAccept) => Some(accept(loop_)),
        Some(summoner::SlashCommand::PlanReject) => Some(reject(workspace)),
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
    let engine = summoner::GoalEngine::open(
        loop_.workspace(),
        objective,
        loop_.session_id().as_str(),
        gates.clone(),
        summoner::DEFAULT_MAX_TURNS,
        loop_.budget(),
    );
    let mut engine = match engine {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("summon: {error}");
            return true;
        }
    };
    loop_.set_verify(None);
    let result = drive_goal(loop_, &mut engine, false).await;
    loop_.set_verify((!gates.is_empty()).then_some(gates));
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
    loop_.set_plan(false);
    eprintln!("plan kept in the session journal — ask to implement it");
    true
}

fn reject(_workspace: &Path) -> bool {
    eprintln!("plan rejected — session keeps the journaled revision");
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
        Event::ToolResult {
            task: Some(task), ..
        } => eprintln!("\n{task}"),
        Event::Verify { passed: true, .. } => eprintln!("\n✓ verify passed"),
        Event::Verify { .. } => eprintln!("\n✗ verify failed"),
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
