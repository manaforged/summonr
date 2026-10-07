use std::path::{Path, PathBuf};

use summonr::{
    Budget, Client, Endpoint, Loop, Result, add_worktree, in_git, last_for, list_sessions,
    list_worktrees, resolve_endpoint, resolve_session, system_prompt, workflow,
};

use super::lifecycle::provider;
use crate::cli::Args;

pub(super) enum SessionAction {
    Create,
    Resume(PathBuf),
    Fork(PathBuf),
}

impl SessionAction {
    fn open(
        self,
        provider: Client,
        model: String,
        workspace: &Path,
        prompt: String,
    ) -> Result<Loop<Client>> {
        match self {
            Self::Create => Loop::new(provider, model, workspace, prompt),
            Self::Resume(path) => Loop::resume_path(provider, model, workspace, prompt, &path),
            Self::Fork(path) => Loop::fork_path(provider, model, workspace, prompt, &path),
        }
    }
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
    ui: Option<&summonr::tui::Ui>,
    json: bool,
) -> Result<workflow::Workflow> {
    let workflow = workflow::load(name)?;
    loop_.apply_workflow(&workflow, |seat| {
        let endpoint = resolve_endpoint(
            Some(&seat.endpoint),
            seat.model.as_deref(),
            summonr::Need::Seat,
        )?;
        let client = provider(&endpoint, ui, json)?;
        Ok((client, endpoint))
    })?;
    Ok(workflow)
}

pub(super) fn isolate(
    args: &Args,
    cwd: &Path,
) -> Result<(PathBuf, Option<summonr::Tree>, SessionAction)> {
    let (action, recorded) = load_session(args, cwd)?;
    match &action {
        SessionAction::Resume(path) | SessionAction::Fork(path) => {
            let workspace = recorded
                .or_else(|| journal_workspace(path))
                .unwrap_or_else(|| cwd.to_path_buf());
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
    let images = summonr::catalog::accepts_images(endpoint);
    let prompt = system_prompt(
        workspace,
        summonr::Sources {
            trust: args.trust,
            skills: switches.enabled(summonr::modules::ModuleId::Skills),
            images,
        },
    );
    let resumed = matches!(action, SessionAction::Resume(_) | SessionAction::Fork(_));
    let mut loop_ = action.open(provider, endpoint.model.clone(), workspace, prompt)?;
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
    loop_.set_effort(
        args.effort
            .or(endpoint.effort)
            .or_else(|| summonr::default_effort(&endpoint.name, &endpoint.model)),
    );
    loop_.set_price(endpoint.price);
    loop_.set_env(endpoint.env.clone());
    loop_.set_images(
        images,
        if images {
            None
        } else {
            summonr::vision_endpoint()?
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
    loop_ = contain(loop_, args)?;
    for dir in &args.add_dir {
        loop_.allow_path(dir)?;
    }
    if args.plan {
        loop_ = loop_.with_plan();
    }
    if args.strict {
        loop_ = loop_.with_strict();
    }
    if loop_.policy_inert() {
        eprintln!(
            "summonr: a policy.toml sets command or write rules, but this session is \
             uncontained; pass --sandbox or --strict to enforce it"
        );
    }
    if let Some(check) = check {
        loop_ = loop_.with_verify(check);
    }
    if let Some(budget) = budget(args) {
        loop_ = loop_.with_budget(budget)?;
    }
    Ok(loop_)
}

fn contain(loop_: Loop<Client>, args: &Args) -> Result<Loop<Client>> {
    if args.plan || args.sandbox.map_or_else(summonr::sandbox_on, Ok)? {
        return loop_.with_sandbox(summonr::Network::allowed(args.network));
    }
    if !args.network {
        return Err(summonr::Error::Config(
            "--no-net needs the sandbox: pass --sandbox".to_string(),
        ));
    }
    Ok(loop_.with_unrestricted_commands())
}

fn budget(args: &Args) -> Option<Budget> {
    if args.max_tokens.is_none() && args.max_secs.is_none() {
        return None;
    }
    Some(Budget {
        max_tokens: args.max_tokens,
        max_secs: args.max_secs,
    })
}

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

fn load_session(args: &Args, workspace: &Path) -> Result<Found> {
    if (args.steps.is_some() || args.goal.is_some()) && args.fork {
        return Err(summonr::Error::Config(
            "cannot fork an active step or goal session".to_string(),
        ));
    }
    if let Some(id) = args.session.as_deref() {
        let path = resolve_session(id)?;
        return Ok((SessionAction::Resume(path), None));
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
    let info = last_for(workspace)?
        .ok_or_else(|| summonr::Error::Config("no prior session for this directory".to_string()))?;
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
            summonr::Error::Config(format!("cannot read {plan_path}: {error}"))
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
