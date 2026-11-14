use std::path::{Path, PathBuf};

use summonr_agent::{
    Client, Loop, Result, last_for, list_sessions, list_worktrees, resolve_session,
};

use super::SessionAction;
use crate::entry::cli::Args;

fn candidates(workspace: &Path) -> Vec<(PathBuf, String)> {
    let mut paths = vec![workspace.to_path_buf()];
    if let Ok(trees) = list_worktrees(workspace) {
        paths.extend(trees.into_iter().map(|tree| tree.path));
    }
    paths
        .into_iter()
        .map(|path| {
            let canonical = dunce::canonicalize(&path).unwrap_or_else(|_| path.clone());
            (path, canonical.display().to_string())
        })
        .collect()
}

type Found = (SessionAction, Option<PathBuf>);

struct Local<'a> {
    id: String,
    path: PathBuf,
    cwd: Option<PathBuf>,
    root: &'a Path,
}

impl Local<'_> {
    fn resume(&self) -> Found {
        (SessionAction::Resume(self.path.clone()), self.cwd.clone())
    }
}

pub(super) fn load_session(args: &Args, workspace: &Path) -> Result<Found> {
    if (args.steps.is_some() || args.goal.is_some()) && args.fork {
        return Err(summonr_agent::Error::Config(
            "cannot fork an active step or goal session".to_string(),
        ));
    }
    if let Some(id) = args.session.as_deref() {
        let path = resolve_session(id)?;
        let action = if args.fork {
            SessionAction::Fork(path)
        } else {
            SessionAction::Resume(path)
        };
        return Ok((action, None));
    }
    if (args.steps.is_some() || args.goal.is_some())
        && let Some(found) = load_job(args, workspace)?
    {
        return Ok(found);
    }
    if args.resume || args.fork {
        return load_last(workspace, args.fork);
    }
    Ok((SessionAction::Create, None))
}

fn load_last(workspace: &Path, fork: bool) -> Result<Found> {
    let info = last_for(workspace)?.ok_or_else(|| {
        summonr_agent::Error::Config("no prior session for this directory".to_string())
    })?;
    let recorded = info.cwd.map(PathBuf::from);
    if fork {
        return Ok((SessionAction::Fork(info.path), recorded));
    }
    Ok((SessionAction::Resume(info.path), recorded))
}

fn local_sessions(roots: &[(PathBuf, String)]) -> Result<Vec<Local<'_>>> {
    Ok(list_sessions()?
        .into_iter()
        .rev()
        .filter_map(|info| {
            let (root, _) = roots
                .iter()
                .find(|(_, canonical)| info.cwd.as_deref() == Some(canonical.as_str()))?;
            Some(Local {
                id: info.id,
                path: info.path,
                cwd: info.cwd.map(PathBuf::from),
                root,
            })
        })
        .collect())
}

fn load_job(args: &Args, workspace: &Path) -> Result<Option<Found>> {
    let markdown = match args.steps.as_deref() {
        Some(plan_path) => Some(std::fs::read_to_string(plan_path).map_err(|error| {
            summonr_agent::Error::Config(format!("cannot read {plan_path}: {error}"))
        })?),
        None => None,
    };
    let roots = candidates(workspace);
    let local = local_sessions(&roots)?;
    if let (Some(plan_path), Some(markdown)) = (args.steps.as_deref(), markdown.as_deref())
        && let Some(hit) = local
            .iter()
            .find(|hit| Loop::<Client>::steps_at(&hit.path, hit.root, markdown))
    {
        eprintln!("resuming steps from {plan_path} in session {}", hit.id);
        return Ok(Some(hit.resume()));
    }
    if let Some(objective) = args.goal.as_deref()
        && let Some(hit) = local
            .iter()
            .find(|hit| Loop::<Client>::goal_at(&hit.path, hit.root, objective))
    {
        eprintln!("resuming goal in session {}", hit.id);
        return Ok(Some(hit.resume()));
    }
    note_active(&local);
    Ok(None)
}

fn note_active(local: &[Local<'_>]) {
    if let Some((hit, (kind, objective))) = local
        .iter()
        .find_map(|hit| Some((hit, Loop::<Client>::job_at(&hit.path, hit.root)?)))
    {
        eprintln!(
            "note: an interrupted {kind:?} run is still active in session {} ({}); rerun it unchanged or `--session {}` to resume it",
            hit.id,
            objective.lines().next().unwrap_or_default(),
            hit.id
        );
    }
}
