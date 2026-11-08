use std::path::{Path, PathBuf};

use summonr_agent::{
    Budget, Client, Endpoint, Loop, Result, add_worktree, in_git, resolve_endpoint, system_prompt,
    workflow,
};

use super::lifecycle::provider;
use crate::entry::cli::Args;

#[path = "session/find.rs"]
mod find;
use find::load_session;

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

/// The skill set a session starts with: `--skills`, else `[skills] default` for a new session.
fn chosen_set(
    args: &Args,
    workspace: &Path,
    resumed: bool,
) -> Result<Option<summonr_agent::skills::Set>> {
    let name = match &args.skills {
        Some(name) => Some(name.clone()),
        None if resumed => None,
        None => summonr_agent::skills::default_set()?,
    };
    name.as_deref()
        .map(|name| summonr_agent::skills::Set::named(name, workspace))
        .transpose()
}

/// Puts the session on `set`, or on the set a resumed journal records, rebuilding the prompt for it.
fn apply_set(
    loop_: &mut Loop<Client>,
    set: Option<summonr_agent::skills::Set>,
    resumed: bool,
    prompt: impl Fn(Option<&summonr_agent::skills::Set>) -> String,
) -> Result<()> {
    let set = match (set, loop_.skills()) {
        (Some(set), _) => set,
        (None, Some(name)) if resumed => {
            let set = summonr_agent::skills::Set::named(&name, loop_.workspace())?;
            loop_.set_system_prompt(prompt(Some(&set)));
            set
        }
        (None, _) => return Ok(()),
    };
    loop_.choose_skills(set)
}

pub(super) fn apply_workflow(
    loop_: &mut Loop<Client>,
    name: &str,
    ui: Option<&summonr_agent::hub::Ui>,
    json: bool,
) -> Result<workflow::Workflow> {
    let workflow = workflow::load(name)?;
    loop_.configure_workflow(&workflow, |selection| {
        let endpoint = resolve_endpoint(
            Some(&selection.endpoint),
            selection.model.as_deref(),
            summonr_agent::Requirement::ModelAndKey,
        )?;
        let client = provider(&endpoint, ui, json)?;
        Ok((client, endpoint))
    })?;
    Ok(workflow)
}

pub(super) fn isolate(
    args: &Args,
    cwd: &Path,
) -> Result<(PathBuf, Option<summonr_agent::Tree>, SessionAction)> {
    let (action, recorded) = load_session(args, cwd)?;
    match &action {
        SessionAction::Resume(path) | SessionAction::Fork(path) => {
            let recorded = recorded.or_else(|| journal_workspace(path));
            let workspace = recorded
                .as_deref()
                .filter(|folder| folder.is_dir())
                .map(Path::to_path_buf)
                .or_else(|| {
                    recorded
                        .as_deref()
                        .and_then(summonr_agent::worktree::checkout_of)
                })
                .unwrap_or_else(|| cwd.to_path_buf());
            let tree = summonr_agent::worktree_at(&workspace);
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

pub(super) fn set_up(
    cwd: &Path,
    tree: Option<&summonr_agent::Tree>,
    action: &SessionAction,
    json: bool,
) -> Result<()> {
    if let (Some(tree), SessionAction::Create) = (tree, action)
        && !same_folder(cwd, &tree.path)
    {
        return setup(tree, json);
    }
    Ok(())
}

fn same_folder(one: &Path, other: &Path) -> bool {
    let real = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    real(one) == real(other)
}

fn setup(tree: &summonr_agent::Tree, json: bool) -> Result<()> {
    use summonr_agent::scripts::{Script, Scripts, Workspace};

    let scripts = Scripts::load(&tree.primary)?;
    let workspace = Workspace::at(&tree.path)?;
    scripts.copy_files(&workspace)?;
    let Some(script) = scripts.get(Script::Setup) else {
        return Ok(());
    };
    let mut command = scripts.command(&workspace, script);
    if json {
        command.stdout(std::io::stderr());
    }
    let status = command.status()?;
    if status.success() {
        return Ok(());
    }
    let ended = status.code().map_or_else(
        || "was stopped by a signal".to_string(),
        |code| format!("exited with code {code}"),
    );
    Err(summonr_agent::Error::Setup(format!(
        "setup script `{script}` {ended} in {}",
        tree.path.display()
    )))
}

fn journal_workspace(path: &Path) -> Option<PathBuf> {
    summonr_agent::session_info(path.to_path_buf())
        .cwd
        .map(PathBuf::from)
}

pub(super) fn build(
    args: &Args,
    endpoint: &Endpoint,
    provider: Client,
    workspace: &Path,
    check: Option<Vec<String>>,
    action: SessionAction,
) -> Result<Loop<Client>> {
    let switches = summonr_agent::modules::load(workspace)?;
    let images = summonr_agent::catalog::accepts_images(endpoint);
    let resumed = matches!(action, SessionAction::Resume(_) | SessionAction::Fork(_));
    let set = chosen_set(args, workspace, resumed)?;
    let prompt = |set: Option<&summonr_agent::skills::Set>| {
        system_prompt(
            workspace,
            summonr_agent::Sources {
                trust: args.trust,
                skills: switches.enabled(summonr_agent::modules::ModuleId::Skills),
                set,
                images,
                memory: switches.enabled(summonr_agent::modules::ModuleId::Memory),
                mcp: switches.enabled(summonr_agent::modules::ModuleId::Mcp),
            },
        )
    };
    let mut loop_ = action.open(
        provider,
        endpoint.model.clone(),
        workspace,
        prompt(set.as_ref()),
    )?;
    if resumed && args.goal.is_none() && args.steps.is_none() {
        eprintln!(
            "resuming session {} in {}",
            summonr_agent::short(&loop_.session_id()),
            loop_.workspace().display()
        );
    }
    if let Some(name) = &args.name {
        loop_.set_name(name)?;
    }
    apply_set(&mut loop_, set, resumed, prompt)?;
    loop_.choose_effort(args.effort);
    loop_.apply_endpoint(endpoint);
    loop_.set_images(images, summonr_agent::vision_endpoint()?);
    loop_.set_modules(&switches)?;
    configure(loop_, args, check)
}

fn configure(
    mut loop_: Loop<Client>,
    args: &Args,
    check: Option<Vec<String>>,
) -> Result<Loop<Client>> {
    loop_ = loop_.with_unrestricted_commands();
    if args.print && args.goal.is_none() && args.steps.is_none() {
        loop_ = loop_.without_task_list();
    }
    for dir in &args.add_dir {
        loop_.allow_path(dir)?;
    }
    if args.strict {
        loop_ = loop_.with_strict();
    }
    if loop_.policy_inert() {
        eprintln!(
            "summonr: a policy.toml sets command or write rules, but this session is \
             unrestricted; pass --strict to enforce it"
        );
    }
    if let Some(check) = check {
        loop_ = loop_.with_verify(check)?;
    }
    if let Some(budget) = budget(args) {
        loop_ = loop_.with_budget(budget)?;
    }
    Ok(loop_)
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
