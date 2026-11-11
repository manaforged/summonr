use std::path::Path;

use summonr_agent::Result;

use super::commands::workspace;
use crate::entry::cli::Args;

pub(super) fn print_diff(args: &Args) -> Result<()> {
    let cwd = workspace()?;
    let path = args.prompt.first().map(String::as_str);
    if let Some(child) = &args.child {
        return print_child(&cwd, child, path, args);
    }
    let turn = args
        .turn
        .as_deref()
        .map(|turn| session_turn(&cwd, turn))
        .transpose()?;
    if turn.is_none() && !args.json && !args.stat {
        print!("{}", summonr_agent::diff::patch(&cwd, path)?);
        return Ok(());
    }
    let mut diff = match &turn {
        Some((session, turn)) => summonr_agent::diff::turn_stats(&cwd, session, *turn)?,
        None => summonr_agent::diff::stats(&cwd)?,
    };
    diff.files
        .retain(|stat| path.is_none_or(|path| stat.path == path));
    let Some((session, turn)) = turn.filter(|_| !args.json && !args.stat) else {
        return print_stats(&diff, args.json);
    };
    eprintln!("{}", diff.summary());
    print!(
        "{}",
        summonr_agent::diff::turn_patch(&cwd, &session, turn, path)?
    );
    Ok(())
}

fn print_child(cwd: &Path, child: &str, path: Option<&str>, args: &Args) -> Result<()> {
    let journal = summonr_agent::last_for(cwd)?
        .map(|info| info.path)
        .ok_or_else(|| summonr_agent::Error::Config(format!("no session for {}", cwd.display())))?;
    let record = summonr_agent::diff::children(&journal)?
        .into_iter()
        .find(|record| record.id.to_string().ends_with(child))
        .ok_or_else(|| {
            summonr_agent::Error::Config(format!("no child `{child}` in the last session"))
        })?;
    let mut delta = summonr_agent::diff::delta(cwd, &record)?;
    delta
        .diff
        .files
        .retain(|stat| path.is_none_or(|path| stat.path == path));
    if args.json || args.stat {
        return print_stats(&delta.diff, args.json);
    }
    eprintln!("{}", delta.diff.summary());
    for stray in &delta.diff.strays {
        eprintln!("not written by the child: {}", stray.path);
    }
    print!("{}", delta.patch(path)?);
    Ok(())
}

fn session_turn(cwd: &Path, turn: &str) -> Result<(String, summonr_agent::diff::TurnId)> {
    let turn = turn.parse()?;
    let session = summonr_agent::last_for(cwd)?
        .map(|info| info.id)
        .ok_or_else(|| summonr_agent::Error::Config(format!("no session for {}", cwd.display())))?;
    Ok((session, turn))
}

fn print_stats(diff: &summonr_agent::diff::Diff, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(diff)?);
        return Ok(());
    }
    for stat in &diff.files {
        let new = if stat.new { "  new" } else { "" };
        println!("+{} −{} {}{new}", stat.added, stat.removed, stat.path);
    }
    for stat in &diff.strays {
        println!("? {} not written by the child", stat.path);
    }
    println!("{}", diff.summary());
    Ok(())
}
