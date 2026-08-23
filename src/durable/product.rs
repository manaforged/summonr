//! Product constructor for format-2 session journals.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::bound::Text;
use super::context::{ContextRole, RequestHead};
use super::convert;
use super::driver::{DriverError, SessionDriver};
use super::fact::LegacySource;
use super::id::{Digest, SessionId, WorkspaceScopeId};
use super::journal;
use super::observe::{ObserveError, workspace_identity_of};
use super::work::WorkStatus;
use super::workspace::{PathKey, WorkspaceScope};
use crate::provider::{Message, Role};
use crate::steps::JobState;

/// Inputs required to open a new format-2 session under a caller home.
pub(crate) struct Open {
    pub home: PathBuf,
    pub workspace: PathBuf,
    pub model: String,
    pub system: String,
    pub endpoint: String,
    pub adapter: String,
}

/// Open a new format-2 session journal and bind the workspace root.
pub(crate) fn open(spec: &Open) -> Result<SessionDriver, DriverError> {
    let session = new_session();
    let scope = scope(session, &spec.workspace)?;
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

/// Read a journaled job snapshot without opening a writer.
pub(crate) fn job_progress(path: &Path) -> Option<JobState> {
    let state = journal::inspect(path).ok()?;
    let snapshot = state
        .works
        .values()
        .find(|work| matches!(work.status, WorkStatus::Active(_)))
        .map(|work| work.projection.as_str())?;
    if !snapshot.trim_start().starts_with('{') {
        return None;
    }
    serde_json::from_str(snapshot).ok()
}

/// Import a validated v1 session log into a create-only format-2 journal.
///
/// Never appends to the v1 file. A second import of the same source bytes
/// resumes the existing format-2 journal.
pub(crate) fn import(
    home: &Path,
    source: &Path,
    workspace: &Path,
) -> Result<SessionDriver, DriverError> {
    let bytes = std::fs::read(source).map_err(|error| DriverError::Import(error.to_string()))?;
    let (replay, unmatched) = crate::session::replay_pending(source)
        .map_err(|error| DriverError::Import(error.to_string()))?;
    let log_digest = Digest::hash(b"mage:legacy-log:v1", &bytes);
    let session = session_from(log_digest);
    let path = journal::session_path(home, session);
    if path.is_file() {
        return resume_matching(home, session, workspace, log_digest);
    }
    let markdown =
        convert::plan(workspace).map_err(|error| DriverError::Import(error.to_string()))?;
    let display = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("legacy");
    let mut legacy = LegacySource {
        display_id: Text::new(display)?,
        format: 1,
        converter: 1,
        log_length: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        log_digest,
        plan_length: None,
        plan_digest: None,
    };
    if let Some(markdown) = markdown.as_ref() {
        legacy.plan_length = Some(u64::try_from(markdown.len()).unwrap_or(u64::MAX));
        legacy.plan_digest = Some(Digest::hash(b"mage:legacy-plan:v1", markdown.as_bytes()));
    }
    let spec = Open {
        home: home.to_path_buf(),
        workspace: workspace.to_path_buf(),
        model: replay
            .model
            .clone()
            .unwrap_or_else(|| "imported".to_string()),
        system: replay.prompt.clone().unwrap_or_default(),
        endpoint: "https://example.test/v1".to_string(),
        adapter: "openai-compatible".to_string(),
    };
    let scope = scope(session, workspace)?;
    let head = request_head(&spec)?;
    let parent = path
        .parent()
        .ok_or_else(|| DriverError::Import("session path has no parent".to_string()))?;
    std::fs::create_dir_all(parent).map_err(|error| DriverError::Import(error.to_string()))?;
    let temp = parent.join(format!(".import-{log_digest}.jsonl"));
    let mut driver = SessionDriver::open_legacy(&temp, session, &scope, head, legacy, workspace)?;
    for message in &replay.history {
        driver.import_entry(context_role(message), &message.content)?;
    }
    for call in unmatched {
        driver.import_uncertain(&call)?;
    }
    if let Some(markdown) = markdown {
        driver.set_work(&markdown)?;
        driver.select_plan(&markdown)?;
    }
    drop(driver);
    std::fs::hard_link(&temp, &path).map_err(|error| DriverError::Import(error.to_string()))?;
    drop(std::fs::remove_file(&temp));
    resume(home, session, workspace)
}

fn resume_matching(
    home: &Path,
    session: SessionId,
    workspace: &Path,
    log_digest: Digest,
) -> Result<SessionDriver, DriverError> {
    let driver = resume(home, session, workspace)?;
    match driver.state().legacy.as_ref() {
        Some(source) if source.log_digest == log_digest => Ok(driver),
        Some(_) => Err(DriverError::Import(
            "sessions-v2 already has a different legacy source".to_string(),
        )),
        None => Err(DriverError::Import(
            "sessions-v2 path exists without a legacy anchor".to_string(),
        )),
    }
}

fn scope(session: SessionId, workspace: &Path) -> Result<WorkspaceScope, DriverError> {
    let identity = workspace_identity_of(workspace)?;
    let platform = identity.platform();
    WorkspaceScope::new(
        WorkspaceScopeId::new(session, 10),
        identity,
        vec![PathKey::root(platform)],
        Vec::new(),
        Vec::new(),
    )
    .map_err(ObserveError::from)
    .map_err(DriverError::from)
}

fn session_from(digest: Digest) -> SessionId {
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    SessionId::from_u128(u128::from_be_bytes(bytes))
}

fn context_role(message: &Message) -> ContextRole {
    match message.role {
        Role::System => ContextRole::System,
        Role::User => ContextRole::User,
        Role::Assistant => ContextRole::Assistant,
        Role::Tool => ContextRole::Tool,
    }
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
        adapter: Text::new(spec.adapter.as_str())?,
        request_format: 1,
        endpoint: Text::new(spec.endpoint.as_str())?,
        model: Text::new(spec.model.as_str())?,
        resource_version: None,
        body_overlay: Text::new("{}")?,
        system: Text::new(spec.system.as_str())?,
        tool_schemas_digest: Digest::hash(b"mage:tool-schema:v1", b""),
        output_settings: Text::new("{}")?,
        context_settings: Text::new("{}")?,
        streaming: true,
        serializer: Text::new("serde-json-1")?,
        digest: Digest::default(),
    };
    head.digest = head.calculated_digest()?;
    Ok(head)
}
