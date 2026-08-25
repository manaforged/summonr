//! Product constructor for format-2 session journals.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::bound::Text;
use super::context::RequestHead;
use super::driver::{DriverError, SessionDriver};
use super::id::{Digest, SessionId, WorkspaceScopeId};
use super::journal;
use super::observe::{ObserveError, workspace_identity_of};
use super::workspace::{
    PathComponent, PathKey, PathPlatform, ReservedCoverage, ReservedPath, WorkspaceScope,
};
use crate::durable::work::{WorkPolicy, WorkStatus};

/// Inputs required to open a new format-2 session under a caller home.
pub(crate) struct Open {
    pub home: PathBuf,
    pub workspace: PathBuf,
    pub model: String,
    pub system: String,
}

/// Open a new format-2 session journal and bind the workspace root.
pub(crate) fn open(spec: &Open) -> Result<SessionDriver, DriverError> {
    let session = new_session();
    let scope = scope(session, &spec.workspace, &spec.home)?;
    let head = request_head(spec)?;
    let path = journal::session_path(&spec.home, session);
    let mut driver = SessionDriver::open(&path, session, &scope, head)?;
    driver.set_workspace(&spec.workspace);
    Ok(driver)
}

/// Resume an existing format-2 session journal and bind the workspace root.
pub(crate) fn resume(
    home: &Path,
    session: SessionId,
    workspace: &Path,
) -> Result<SessionDriver, DriverError> {
    let path = journal::session_path(home, session);
    let mut driver = SessionDriver::resume(&path, session)?;
    driver.set_workspace(workspace);
    driver.recover()?;
    Ok(driver)
}

/// Whether a journal contains the active external Work requested by this
/// workspace and invocation.
pub(crate) fn work_matches(
    path: &Path,
    workspace: &Path,
    policy: WorkPolicy,
    objective: &str,
) -> bool {
    let Ok(state) = journal::inspect(path) else {
        return false;
    };
    let scope = state.default_scope.and_then(|id| state.scopes.get(&id));
    let identity = workspace_identity_of(workspace).ok();
    if scope.map(|scope| &scope.workspace) != identity.as_ref() {
        return false;
    }
    state.works.values().any(|work| {
        work.policy() == policy
            && work.objective.as_str() == objective
            && !matches!(
                work.status(),
                WorkStatus::Completed { .. } | WorkStatus::Abandoned { .. }
            )
    })
}

fn scope(session: SessionId, workspace: &Path, home: &Path) -> Result<WorkspaceScope, DriverError> {
    let identity = workspace_identity_of(workspace)?;
    let platform = identity.platform();
    WorkspaceScope::new(
        WorkspaceScopeId::new(session, 10),
        identity,
        vec![PathKey::root(platform)],
        ignored_directories(workspace, home, platform)?,
        Vec::new(),
    )
    .map_err(ObserveError::from)
    .map_err(DriverError::from)
}

fn ignored_directories(
    workspace: &Path,
    home: &Path,
    platform: PathPlatform,
) -> Result<Vec<ReservedPath>, DriverError> {
    let mut paths = vec![
        relative_path(platform, &[".git"])?,
        relative_path(platform, &[".mage"])?,
    ];
    if same_directory(workspace, home) {
        paths.push(relative_path(platform, &["sessions-v2"])?);
    }
    if let Ok(text) = std::fs::read_to_string(workspace.join(".gitignore")) {
        let lines = text.lines().map(str::trim).collect::<Vec<_>>();
        let mut negated = Vec::new();
        for line in lines.iter().filter_map(|line| line.strip_prefix('!')) {
            let Some(parts) = ignore_parts(line) else {
                return reserved(paths);
            };
            negated.push(relative_path(platform, &parts)?);
        }
        for line in lines {
            if line.starts_with(['#', '!']) {
                continue;
            }
            let Some(parts) = ignore_parts(line) else {
                continue;
            };
            let path = relative_path(platform, &parts)?;
            if !negated.iter().any(|negated| path.prefixes(negated)) {
                paths.push(path);
            }
        }
    }
    reserved(paths)
}

fn ignore_parts(pattern: &str) -> Option<Vec<&str>> {
    if pattern.is_empty() || pattern.contains(['*', '?', '[', ']']) {
        return None;
    }
    let parts = pattern.trim_matches('/').split('/').collect::<Vec<_>>();
    (!parts.is_empty() && parts.iter().all(|part| !part.is_empty())).then_some(parts)
}

fn reserved(mut paths: Vec<PathKey>) -> Result<Vec<ReservedPath>, DriverError> {
    paths.sort();
    paths.dedup();
    let mut roots: Vec<PathKey> = Vec::new();
    for path in paths {
        if roots.iter().any(|root| root.prefixes(&path)) {
            continue;
        }
        roots.push(path);
    }
    Ok(roots
        .into_iter()
        .map(|path| ReservedPath::new(path, ReservedCoverage::Subtree))
        .collect())
}

fn same_directory(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn relative_path(platform: PathPlatform, parts: &[&str]) -> Result<PathKey, DriverError> {
    let components = parts
        .iter()
        .map(|part| match platform {
            PathPlatform::Unix => PathComponent::unix(part.as_bytes().to_vec()),
            PathPlatform::Windows => PathComponent::windows(part.encode_utf16().collect()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PathKey::new(platform, components)?)
}

fn new_session() -> SessionId {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    SessionId::from_u128(nanos ^ u128::from(std::process::id()))
}

fn request_head(spec: &Open) -> Result<RequestHead, DriverError> {
    let mut head = RequestHead {
        model: Text::new(spec.model.as_str())?,
        system: Text::new(spec.system.as_str())?,
        digest: Digest::default(),
    };
    head.digest = head.calculated_digest()?;
    Ok(head)
}
