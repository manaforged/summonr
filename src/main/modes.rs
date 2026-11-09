use summonr::{
    Client, Event, Job, Loop, NextAction, Result, StepState, Turn, UserInput, WorkKind,
    WorkProgress,
};
use tokio::io::{self, AsyncReadExt};

use super::cli::Args;

pub(super) async fn print(mut loop_: Loop<Client>, args: &Args) -> Result<i32> {
    let prompt = if args.prompt.is_empty() {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input).await?;
        input
    } else {
        args.prompt.join(" ")
    };
    if let Some(command) = slash(&prompt) {
        return Err(summonr::Error::Config(format!(
            "`{command}` is an interface command; -p takes a prompt"
        )));
    }
    let input = summonr::attach(loop_.workspace(), &prompt)?;
    announce(&loop_, args.json);
    let turn = complete(&mut loop_, input, args.json).await?;
    if !args.json {
        println!();
    }
    let progress = loop_.work_progress()?;
    Ok(exit(&turn, &progress))
}

pub(super) async fn steps(mut loop_: Loop<Client>, args: &Args, plan_path: &str) -> Result<i32> {
    let path = std::path::absolute(plan_path)?;
    let (plan, markdown, steps) = summonr::steps::load(loop_.workspace(), &path)?;
    let checks = loop_.verify_commands().to_vec();
    loop_.select_steps(&markdown, &checks)?;
    work(loop_, args, Job::Steps { plan, steps }).await
}

pub(super) async fn goal(mut loop_: Loop<Client>, args: &Args, objective: &str) -> Result<i32> {
    let gates = loop_.verify_commands().to_vec();
    loop_.select_goal(objective, &gates)?;
    let objective = objective.to_string();
    work(loop_, args, Job::Goal { objective, gates }).await
}

async fn work(mut loop_: Loop<Client>, args: &Args, job: Job) -> Result<i32> {
    announce(&loop_, args.json);
    let code = loop {
        let (prompt, step) = match loop_.next_action(&job)? {
            NextAction::Done(code) => break code,
            NextAction::Run { prompt, step } => (prompt, step),
        };
        if let Some((index, total)) = step {
            step_event(
                args.json,
                index,
                total,
                StepState::Start,
                job.check(index).as_deref(),
            );
        }
        let turn = complete(&mut loop_, prompt.into(), args.json).await?;
        if turn.aborted {
            return stop(&mut loop_, job.aborted(), args.json).await;
        }
        if let Some((index, total)) = step {
            step_outcome(args.json, index, total, &turn, &loop_.work_progress()?);
        }
    };
    if !args.json {
        println!();
    }
    Ok(code)
}

fn step_outcome(json: bool, index: usize, total: usize, turn: &Turn, after: &WorkProgress) {
    if let Some(check) = turn
        .checks
        .iter()
        .find(|check| !check.passed)
        .or_else(|| turn.checks.last())
    {
        step_event(
            json,
            index,
            total,
            if check.passed {
                StepState::CheckPassed
            } else {
                StepState::CheckFailed
            },
            Some(&check.command),
        );
    } else if after.current != Some(index) || after.terminal() {
        step_event(json, index, total, StepState::Done, None);
    }
}

fn announce(loop_: &Loop<Client>, json: bool) {
    if json {
        print_json(&Event::Session {
            id: loop_.session_id(),
        });
    }
}

fn printer(json: bool) -> fn(&Event) {
    if json { print_json } else { print_live }
}

async fn stop(loop_: &mut Loop<Client>, reason: &str, json: bool) -> Result<i32> {
    loop_.stop_work(reason, printer(json)).await
}

async fn complete(loop_: &mut Loop<Client>, input: UserInput, json: bool) -> Result<Turn> {
    loop_.turn_with(input, printer(json)).await
}

fn slash(prompt: &str) -> Option<&str> {
    let line = prompt.trim_start().lines().next()?;
    summonr::parse_command(line)?;
    line.split_whitespace().next()
}

pub(super) fn print_json(event: &Event) {
    match serde_json::to_string(event) {
        Ok(line) => println!("{line}"),
        Err(error) => eprintln!("cannot encode event: {error}"),
    }
}

fn print_live(event: &Event) {
    match event {
        Event::ToolCall {
            name, arguments, ..
        } => eprintln!("\n→ {}", summonr::mcp_call_label(name, arguments)),
        Event::ToolResult { success: false, .. } => eprintln!("  (failed)"),
        Event::Verify { passed: true, .. } => eprintln!("\n✓ verify passed"),
        Event::Verify { .. } => eprintln!("\n✗ verify failed"),
        Event::Work { progress } if progress.kind != WorkKind::Single => {
            eprintln!("\n{progress}");
            for line in &progress.ledger {
                eprintln!("{line}");
            }
        }
        Event::Receipt { .. } => {}
        Event::Aborted { reason, .. } => eprintln!("\n{reason}"),
        Event::Child { id, event } => {
            let tag = summonr::child::tag(id);
            match &**event {
                Event::ToolCall {
                    name, arguments, ..
                } => eprintln!("  [{tag}] → {}", summonr::mcp_call_label(name, arguments)),
                Event::ToolResult { success: false, .. } => eprintln!("  [{tag}] (failed)"),
                Event::Assistant { text } => {
                    eprintln!("  [{tag}] {}", text.lines().next().unwrap_or_default());
                }
                Event::Aborted { reason, .. } => eprintln!("  [{tag}] {reason}"),
                _ => {}
            }
        }
        _ => {}
    }
}

fn exit(turn: &Turn, progress: &WorkProgress) -> i32 {
    progress.exit_status(turn.checks.iter().any(|check| !check.passed), turn.aborted)
}

fn step_event(json: bool, index: usize, total: usize, state: StepState, check: Option<&str>) {
    if json {
        print_json(&Event::Step {
            index,
            total,
            state,
            check: check.map(str::to_string),
        });
    } else if let Some(check) = check {
        eprintln!("\n[step {index}/{total}] {} · {check}", state.as_str());
    } else {
        eprintln!("\n[step {index}/{total}] {}", state.as_str());
    }
}
