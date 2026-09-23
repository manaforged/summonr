use std::path::{Path, PathBuf};

use summonr::{
    Budget, Client, Endpoint, Loop, Result, add_worktree, in_git, last_for, list_sessions,
    list_worktrees, resolve_endpoint, resolve_session, system_prompt_with, workflow,
};
use tokio::sync::mpsc::UnboundedSender;

use super::lifecycle::provider;
use crate::cli::Args;

pub(super) enum SessionAction {
    Create,
    Resume(PathBuf),
    Fork(PathBuf),
}

pub(super) fn workflow_name(args: &Args, loop_: &Loop<Client>) -> Result<Option<String>> {
    if let Some(name) = &args.workflow {
        return Ok(Some(name.clone()));
    }
    if let Some(name) = loop_.workflow() {
        return Ok(Some(name));
    }
    workflow::default_name()
}

pub(super) fn apply_workflow(
    loop_: &mut Loop<Client>,
    name: &str,
    ui: Option<&UnboundedSender<summonr::tui::Msg>>,
    json: bool,
) -> Result<workflow::Workflow> {
    let workflow = workflow::load(name)?;
    loop_.apply_workflow(&workflow, |seat| {
        let endpoint = resolve_endpoint(Some(&seat.endpoint), seat.model.as_deref())?;
        let client = provider(&endpoint, ui, json)?;
        Ok((client, endpoint))
    })?;
    Ok(workflow)
}

pub(super) fn isolate(
    args: &Args,
    cwd: &Path,
) -> Result<(PathBuf, Option<summonr::Tree>, SessionAction)> {
    let action = load_session(args, cwd)?;
    match &action {
        SessionAction::Resume(path) | SessionAction::Fork(path) => {
            let workspace = journal_workspace(path).unwrap_or_else(|| cwd.to_path_buf());
            let tree = list_worktrees(&workspace)
                .ok()
                .and_then(|trees| trees.into_iter().find(|tree| tree.path == workspace));
            Ok((workspace, tree, action))
        }
        SessionAction::Create if args.worktree => {
            if !in_git(cwd) {
                if !args.json {
                    eprintln!("not a git repository; running in {}", cwd.display());
                }
                return Ok((cwd.to_path_buf(), None, action));
            }
            let tree = add_worktree(cwd)?;
            if !args.json {
                println!("worktree {} ({})", tree.path.display(), tree.branch);
            }
            Ok((tree.path.clone(), Some(tree), action))
        }
        SessionAction::Create => Ok((cwd.to_path_buf(), None, action)),
    }
}

fn journal_workspace(path: &Path) -> Option<PathBuf> {
    list_sessions()
        .ok()?
        .into_iter()
        .find(|info| info.path == path)?
        .cwd
        .map(PathBuf::from)
}

pub(super) fn route_checks(
    goal: bool,
    checks: Option<Vec<String>>,
) -> (Option<Vec<String>>, Vec<String>) {
    if goal {
        (checks.clone(), checks.unwrap_or_default())
    } else {
        (checks, Vec::new())
    }
}

pub(super) fn build(
    args: &Args,
    endpoint: &Endpoint,
    provider: Client,
    workspace: &Path,
    check: Option<Vec<String>>,
    action: SessionAction,
) -> Result<Loop<Client>> {
    let switches = summonr::modules::Switches::load(workspace)?;
    let prompt = system_prompt_with(
        workspace,
        args.trust,
        switches.enabled("skills"),
        endpoint.images,
    );
    let resumed = matches!(action, SessionAction::Resume(_) | SessionAction::Fork(_));
    let mut loop_ = match action {
        SessionAction::Create => Loop::new(provider, endpoint.model.clone(), workspace, prompt)?,
        SessionAction::Resume(path) => {
            Loop::resume_path(provider, endpoint.model.clone(), workspace, prompt, &path)?
        }
        SessionAction::Fork(path) => {
            Loop::fork_path(provider, endpoint.model.clone(), workspace, prompt, &path)?
        }
    };
    if resumed && args.goal.is_none() && args.steps.is_none() {
        eprintln!(
            "resuming session {} in {}",
            loop_.session_id(),
            loop_.workspace().display()
        );
    }
    if let Some(name) = &args.name {
        loop_.set_name(name)?;
    }
    loop_.set_context_window(endpoint.context_window);
    loop_.set_compact_tokens(endpoint.compact_tokens);
    loop_.set_effort(args.effort.or(endpoint.effort));
    loop_.set_price(endpoint.price);
    loop_.set_env(endpoint.env.clone());
    loop_.set_images(
        endpoint.images,
        if endpoint.images {
            None
        } else {
            summonr::vision_endpoint()
        },
    );
    loop_.set_modules(&switches);
    configure(loop_, args, check)
}

fn configure(
    mut loop_: Loop<Client>,
    args: &Args,
    check: Option<Vec<String>>,
) -> Result<Loop<Client>> {
    if args.sandbox || args.plan || summonr::sandbox_on()? {
        loop_ = loop_.with_sandbox(!args.network)?;
    } else {
        if !args.network {
            eprintln!("summonr: --no-net needs --sandbox; sockets stay open");
        }
        loop_ = loop_.with_unrestricted_commands();
    }
    for dir in &args.add_dir {
        loop_.allow_path(dir)?;
    }
    if args.plan {
        loop_ = loop_.with_plan();
    }
    if args.strict {
        loop_ = loop_.with_strict();
    }
    if let Some(check) = check {
        loop_ = loop_.with_verify(check);
    }
    let budget = if args.max_tokens.is_some() || args.max_secs.is_some() {
        Some(Budget {
            max_tokens: args.max_tokens,
            max_secs: args.max_secs,
        })
    } else {
        None
    };
    if let Some(budget) = budget {
        loop_ = loop_.with_budget(budget)?;
    }
    Ok(loop_)
}

fn candidates(workspace: &Path) -> Vec<PathBuf> {
    let mut paths = vec![workspace.to_path_buf()];
    if let Ok(trees) = list_worktrees(workspace) {
        paths.extend(trees.into_iter().map(|tree| tree.path));
    }
    paths
}

fn load_session(args: &Args, workspace: &Path) -> Result<SessionAction> {
    if (args.steps.is_some() || args.goal.is_some()) && args.fork {
        return Err(summonr::Error::Config(
            "cannot fork an active step or goal session".to_string(),
        ));
    }
    if let Some(id) = args.session.as_deref() {
        let path = resolve_session(id)?;
        return Ok(SessionAction::Resume(path));
    }
    if let Some(plan_path) = args.steps.as_deref() {
        let markdown = std::fs::read_to_string(plan_path)
            .map_err(|error| summonr::Error::Config(format!("cannot read {plan_path}: {error}")))?;
        let roots = candidates(workspace);
        if let Some(info) = list_sessions()?.into_iter().rev().find(|info| {
            roots
                .iter()
                .any(|root| Loop::<Client>::steps_at(&info.path, root, &markdown))
        }) {
            eprintln!("resuming steps from {plan_path} in session {}", info.id);
            return Ok(SessionAction::Resume(info.path));
        }
    }
    if let Some(objective) = args.goal.as_deref() {
        let roots = candidates(workspace);
        if let Some(info) = list_sessions()?.into_iter().rev().find(|info| {
            roots
                .iter()
                .any(|root| Loop::<Client>::goal_at(&info.path, root, objective))
        }) {
            eprintln!("resuming goal in session {}", info.id);
            return Ok(SessionAction::Resume(info.path));
        }
    }
    if args.steps.is_some() || args.goal.is_some() {
        let roots = candidates(workspace);
        for info in list_sessions()?.into_iter().rev() {
            if let Some((kind, objective)) = roots
                .iter()
                .find_map(|root| Loop::<Client>::job_at(&info.path, root))
            {
                eprintln!(
                    "note: an interrupted {kind:?} run is still active in session {} — {} — rerun it unchanged or `--session {}` to resume it",
                    info.id,
                    objective.lines().next().unwrap_or_default(),
                    info.id
                );
                break;
            }
        }
    }
    if args.resume || args.fork {
        let path = last_for(workspace)?.ok_or_else(|| {
            summonr::Error::Config("no prior session for this directory".to_string())
        })?;
        if args.fork {
            return Ok(SessionAction::Fork(path));
        }
        return Ok(SessionAction::Resume(path));
    }
    Ok(SessionAction::Create)
}
