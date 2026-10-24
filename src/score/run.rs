use std::collections::BTreeMap;
use std::path::Path;

use super::{Board, Entry, Goal, Reading, Spend, ledger, parse, spend};
use crate::error::Result;
use crate::tools::ToolRegistry;
use crate::verify::ScoreConfig;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Measured {
    Scored(Board),
    Failed { board: Board, reason: String },
}

impl Measured {
    #[must_use]
    pub(crate) fn board(&self) -> &Board {
        match self {
            Self::Scored(board) | Self::Failed { board, .. } => board,
        }
    }

    #[must_use]
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Scored(board) => board.text(),
            Self::Failed { board, reason } => {
                format!("Score command failed: {reason}\n{}", board.text())
            }
        }
    }

    #[must_use]
    pub(crate) fn met(&self) -> bool {
        match self {
            Self::Scored(board) => board.met,
            Self::Failed { board, .. } => board.rows.iter().all(|row| row.target.is_none()),
        }
    }
}

async fn scores(
    tools: &ToolRegistry,
    command: &str,
) -> std::result::Result<BTreeMap<String, f64>, String> {
    let result = tools.run_check(command).await;
    let outcome = crate::verify::outcome(command, &result);
    if !outcome.passed {
        return Err(outcome.output.trim().to_string());
    }
    parse(&outcome.output).ok_or_else(|| {
        "the score command printed no `name value` lines or JSON object of numbers".to_string()
    })
}

fn failed(entries: &[Entry], session: Option<&str>, goal: &Goal) -> usize {
    entries
        .iter()
        .rev()
        .filter(|entry| goal.owns(entry, session))
        .take_while(|entry| entry.error.is_some())
        .count()
}

pub(crate) async fn measure(
    tools: &ToolRegistry,
    workspace: &Path,
    config: &ScoreConfig,
    session: Option<String>,
    goal: &mut Goal,
    watched: Option<Spend>,
) -> Result<Measured> {
    let measured = match &config.command {
        Some(command) => scores(tools, command).await,
        None => Ok(BTreeMap::new()),
    };
    let path = ledger::path(workspace)?;
    let mut entries = ledger::read(&path)?;
    let mut spend = match watched {
        Some(spend) => Some(spend),
        None => spend::read(tools, config, &entries, session.as_deref(), goal).await,
    };
    if let Some(spend) = &mut spend {
        spend::halt(tools, config, spend).await;
    }
    let (scores, failure) = match measured {
        Ok(scores) => (scores, None),
        Err(reason) => (BTreeMap::new(), Some(reason)),
    };
    let mut entry = Entry::new(scores, failure.clone(), session.clone(), workspace);
    if let Some(spend) = &spend {
        spend.record(&mut entry);
    }
    ledger::append(&path, &entry)?;
    entries.push(entry);
    let mut board = Board::new(config, path, &entries);
    board.spend = spend;
    board.failed = failed(&entries, session.as_deref(), goal);
    if config.command.is_some() {
        match (&failure, entries.last()) {
            (None, Some(entry)) => goal.observe(
                super::tree(workspace, session.as_deref()),
                Reading::Scores(entry.scores.clone()),
            ),
            (Some(_), _) | (None, None) => goal.quiet += 1,
        }
        board.quiet = goal.quiet;
    }
    Ok(match failure {
        Some(reason) => Measured::Failed { board, reason },
        None => Measured::Scored(board),
    })
}

pub(crate) async fn watch(
    tools: &ToolRegistry,
    workspace: &Path,
    config: &ScoreConfig,
    session: Option<&str>,
    goal: &mut Goal,
) -> Result<Option<Spend>> {
    if config.spend.is_none() || !goal.spend_due() {
        return Ok(None);
    }
    let entries = ledger::read(&ledger::path(workspace)?)?;
    let mut spend = spend::read(tools, config, &entries, session, goal).await;
    if let Some(spend) = &mut spend {
        spend::halt(tools, config, spend).await;
    }
    Ok(spend)
}

pub(crate) fn board(
    workspace: &Path,
    config: &ScoreConfig,
    session: Option<&str>,
    goal: &Goal,
) -> Result<Board> {
    let path = ledger::path(workspace)?;
    let entries = ledger::read(&path)?;
    let mut board = Board::new(config, path, &entries);
    board.failed = failed(&entries, session, goal);
    board.quiet = goal.quiet;
    Ok(board)
}
