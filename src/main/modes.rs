use summonr::{Client, Event, Loop, Result, Turn, UserInput, WorkKind, WorkProgress};
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
    let markdown = std::fs::read_to_string(plan_path)
        .map_err(|error| summonr::Error::Config(format!("cannot read {plan_path}: {error}")))?;
    let steps = summonr::steps::parse(&markdown)?;
    let checks = loop_.verify_commands().to_vec();
    loop_.select_steps(&markdown, &checks)?;
    announce(&loop_, args.json);
    loop {
        let (index, total, prompt) = match loop_.next_step(&steps, plan_path)? {
            summonr::StepRun::Done(code) => {
                if !args.json {
                    println!();
                }
                return Ok(code);
            }
            summonr::StepRun::Run {
                index,
                total,
                prompt,
            } => (index, total, prompt),
        };
        step_event(args.json, index, total, "start", None);
        let turn = complete(&mut loop_, prompt.into(), args.json).await?;
        if turn.aborted {
            return stop(&mut loop_, "step execution aborted", args.json).await;
        }
        let after = loop_.work_progress()?;
        step_outcome(args.json, index, total, &turn, &after);
    }
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
                "check passed"
            } else {
                "check failed"
            },
            Some(&check.command),
        );
    } else if after.current != Some(index) || after.terminal() {
        step_event(json, index, total, "done", None);
    }
}

pub(super) async fn goal(
    mut loop_: Loop<Client>,
    args: &Args,
    objective: &str,
    gates: Vec<String>,
) -> Result<i32> {
    loop_.select_goal(objective, &gates)?;
    announce(&loop_, args.json);
    let code = drive_goal(&mut loop_, objective, &gates, args.json).await?;
    if !args.json {
        println!();
    }
    Ok(code)
}

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
            summonr::goal_prompt(objective, gates, progress.rounds).into(),
            json,
        )
        .await?;
        if turn.aborted {
            return stop(loop_, "goal execution aborted", json).await;
        }
    }
}

fn announce(loop_: &Loop<Client>, json: bool) {
    if json {
        print_json(&Event::Session {
            id: loop_.session_id(),
        });
    }
}

async fn stop(loop_: &mut Loop<Client>, reason: &str, json: bool) -> Result<i32> {
    if json {
        loop_.stop_work(reason, print_json).await
    } else {
        loop_.stop_work(reason, print_live).await
    }
}

async fn complete(loop_: &mut Loop<Client>, input: UserInput, json: bool) -> Result<Turn> {
    if json {
        loop_.turn_with(input, print_json).await
    } else {
        loop_.turn_with(input, print_live).await
    }
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
        Event::Receipt { receipt } => println!("\n{}", receipt.text),
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
    if turn.checks.iter().any(|check| !check.passed) {
        2
    } else if turn.aborted {
        3
    } else {
        progress.exit_code().unwrap_or(0)
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
