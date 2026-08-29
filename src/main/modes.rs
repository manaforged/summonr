use summoner::{Client, Event, Loop, Result, Turn, UserInput};
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
        return Err(summoner::Error::Config(format!(
            "`{command}` is an interface command; -p takes a prompt"
        )));
    }
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

async fn complete(loop_: &mut Loop<Client>, input: UserInput, json: bool) -> Result<Turn> {
    if json {
        loop_.turn_with(input, print_json).await
    } else {
        loop_.turn_with(input, print_live).await
    }
}

fn slash(prompt: &str) -> Option<&str> {
    let line = prompt.trim_start().lines().next()?;
    summoner::parse_command(line)?;
    line.split_whitespace().next()
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
        Event::Work { progress } => {
            eprintln!("\n{progress}");
            for line in &progress.ledger {
                eprintln!("{line}");
            }
        }
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

#[cfg(test)]
#[path = "modes_tests.rs"]
mod tests;
