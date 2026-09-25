use std::borrow::Cow;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::policy::{Grant, Policy};
use crate::provider::{ToolCall, ToolFunction, ToolKind, UserInput};
use crate::web::Search;

mod args;
mod event;
mod help;
pub use event::Event;
mod catalog;
mod dedup;
pub(crate) use catalog::{Builtin, readonly, retired_op};
pub(crate) mod command;
mod lsp;
mod paths;
pub(crate) mod sandbox;
mod search;
mod todo;
mod workspace;

pub(crate) use args::tool;
use args::{nonempty_path, object_bool, object_string, object_usize, parse_args};
use paths::volume_unmounted;
use todo::parse_todo;
pub(crate) use todo::{TodoStatus, todos_from_args};

pub(crate) fn command_exit_code(output: &str) -> Option<i32> {
    command::exit_code(output)
}

pub(crate) struct ToolRegistry {
    workspace: PathBuf,
    policy: Policy,
    search: Option<Search>,
    env: Vec<(String, String)>,
    sandbox: Option<sandbox::Sandbox>,
    background: bool,
    language: bool,
    skills: bool,
    probed: std::sync::OnceLock<(bool, bool)>,
    lsp: std::sync::Mutex<
        std::collections::BTreeMap<String, std::sync::Arc<tokio::sync::Mutex<Option<lsp::Conn>>>>,
    >,
    session: std::sync::Mutex<Option<String>>,
    read_ranges: std::sync::Mutex<dedup::Ranges>,
    logs: std::sync::Mutex<Vec<tempfile::TempDir>>,
    network_once: std::sync::atomic::AtomicBool,
    approved_once: std::sync::atomic::AtomicBool,
    allowed_edits: Vec<&'static str>,
    model_images: bool,
    vision: Option<crate::endpoint::Endpoint>,
}

pub(crate) struct ToolOutput {
    pub(crate) text: String,
    pub(crate) success: bool,
    pub(crate) mutated: bool,
    pub(crate) grant: Option<Grant>,
}

impl ToolOutput {
    pub(crate) fn ok(text: String) -> Self {
        Self {
            text,
            success: true,
            mutated: false,
            grant: None,
        }
    }
}

pub(crate) fn needed_grant(result: &Result<ToolOutput>) -> Option<Grant> {
    match result {
        Ok(output) => output.grant.clone(),
        Err(error) => crate::policy::grant_from_error(error),
    }
}

impl ToolRegistry {
    pub(crate) fn set_env(&mut self, env: impl IntoIterator<Item = (String, String)>) {
        self.env = env.into_iter().collect();
    }

    pub(crate) fn with_policy_and_search(
        workspace: impl AsRef<Path>,
        policy: Policy,
        search: Option<Search>,
    ) -> Result<Self> {
        let workspace = workspace
            .as_ref()
            .canonicalize()
            .map_err(|error| Error::Tool(format!("cannot open workspace: {error}")))?;
        if !workspace.is_dir() {
            return Err(Error::Tool("workspace is not a directory".to_string()));
        }
        Ok(Self {
            workspace,
            policy,
            search,
            env: Vec::new(),
            sandbox: None,
            background: true,
            language: true,
            skills: true,
            probed: std::sync::OnceLock::new(),
            lsp: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            session: std::sync::Mutex::new(None),
            read_ranges: std::sync::Mutex::new(std::collections::HashMap::new()),
            logs: std::sync::Mutex::new(Vec::new()),
            network_once: std::sync::atomic::AtomicBool::new(false),
            approved_once: std::sync::atomic::AtomicBool::new(false),
            allowed_edits: Vec::new(),
            model_images: false,
            vision: None,
        })
    }

    pub(crate) fn set_images(
        &mut self,
        model_images: bool,
        vision: Option<crate::endpoint::Endpoint>,
    ) {
        self.model_images = model_images;
        self.vision = vision;
    }

    pub(crate) fn set_modules(&mut self, search: bool, lsp: bool, jobs: bool, skills: bool) {
        if !search {
            self.search = None;
        }
        self.language = lsp;
        self.background = jobs;
        self.skills = skills;
        self.probed = std::sync::OnceLock::new();
    }

    pub(crate) fn set_strict(&mut self) {
        self.policy.set_strict();
    }

    pub(crate) fn set_unrestricted(&mut self) {
        self.policy.set_unrestricted();
    }

    pub(crate) fn child_at(&self, workspace: &Path, session: &str) -> Result<Self> {
        let mut child = Self::with_policy_and_search(workspace, self.policy.clone(), None)?;
        child.env.clone_from(&self.env);
        child.sandbox = self
            .sandbox
            .as_ref()
            .map(|sandbox| sandbox.at(&child.workspace))
            .transpose()?;
        child.background = false;
        child.language = self.language;
        child.allowed_edits.clone_from(&self.allowed_edits);
        child.skills = self.skills;
        child.set_session(session);
        Ok(child)
    }

    pub(crate) fn exists(&self, path: &str) -> Result<bool> {
        Ok(self.lexical(path)?.exists())
    }

    pub(crate) fn enable_sandbox(&mut self, deny_network: bool) -> Result<()> {
        self.sandbox = Some(sandbox::Sandbox::discover(
            &self.workspace,
            deny_network,
            &self.policy.write_roots(),
        )?);
        Ok(())
    }

    pub(crate) fn allow_path(&mut self, path: PathBuf) -> Result<()> {
        if !path.exists() {
            if volume_unmounted(&path) {
                return Err(Error::Config(format!(
                    "write path {}: volume is not mounted",
                    path.display()
                )));
            }
            std::fs::create_dir_all(&path).map_err(|error| {
                Error::Config(format!(
                    "cannot create write path {}: {error}",
                    path.display()
                ))
            })?;
        }
        let resolved = path.canonicalize().map_err(|error| {
            Error::Config(format!(
                "write path {} is unusable: {error}",
                path.display()
            ))
        })?;
        if !resolved.is_dir() {
            return Err(Error::Config(format!(
                "write path {} is not a directory",
                resolved.display()
            )));
        }
        self.policy.allow_path(resolved);
        if self.sandbox.is_some() {
            let deny = self
                .sandbox
                .as_ref()
                .is_some_and(sandbox::Sandbox::denies_network);
            self.enable_sandbox(deny)?;
        }
        Ok(())
    }

    pub(crate) fn write_roots(&self) -> Vec<String> {
        self.policy
            .write_roots()
            .iter()
            .map(|path| path.display().to_string())
            .collect()
    }

    pub(crate) fn allow_network_once(&self) {
        self.network_once
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn approve_once(&self) {
        self.approved_once
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn take_approval(&self) -> bool {
        self.approved_once
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn has_sandbox(&self) -> bool {
        self.sandbox.is_some()
    }

    pub(crate) fn denies_network(&self) -> bool {
        self.sandbox
            .as_ref()
            .is_some_and(sandbox::Sandbox::denies_network)
    }

    pub(crate) fn is_strict(&self) -> bool {
        self.policy.strict
    }

    pub(crate) fn is_read_only(&self) -> bool {
        self.policy.is_read_only()
    }

    pub(crate) fn set_session(&self, id: &str) {
        if let Ok(mut session) = self.session.lock() {
            *session = Some(id.to_string());
        }
    }

    pub(crate) fn set_read_only(&mut self, on: bool) {
        self.policy.set_read_only(on);
    }

    pub(crate) async fn execute_outcome(&self, call: &ToolCall) -> Result<ToolOutput> {
        let call = normalize(call)?;
        match Builtin::live(&call.function.name) {
            Some(kind) if self.enabled(kind) => self.dispatch(kind, &call).await,
            Some(kind) if kind != Builtin::Task => Err(Error::Denied(format!(
                "tool `{}` is unavailable in the current mode",
                call.function.name
            ))),
            Some(_) | None => Err(Error::Tool(format!(
                "unknown tool `{}`",
                call.function.name
            ))),
        }
    }

    async fn dispatch(&self, kind: Builtin, call: &ToolCall) -> Result<ToolOutput> {
        let arguments = &call.function.arguments;
        match kind {
            Builtin::Read => tokio::task::block_in_place(|| self.read(arguments))
                .map(ToolOutput::ok)
                .map(|out| self.dedup_read(arguments, out)),
            Builtin::Edit => self.edit_file(arguments).await,
            Builtin::Write => self.write_file(arguments).await,
            Builtin::Skill => self.skill(arguments).map(ToolOutput::ok),
            Builtin::RunCommand => self.run_command(arguments).await,
            Builtin::Todo => parse_todo(arguments).map(ToolOutput::ok),
            Builtin::Web => self.web(arguments).await.map(ToolOutput::ok),
            Builtin::Lsp => self.lsp(arguments).await,
            Builtin::Task => Err(Error::Tool(format!(
                "unknown tool `{}`",
                call.function.name
            ))),
        }
    }
}

pub struct ToolResult {
    pub text: String,
    pub success: bool,
}

pub async fn run_tool(
    workspace: impl AsRef<Path>,
    name: &str,
    arguments: &str,
    sandbox: bool,
    deny_network: bool,
) -> Result<ToolResult> {
    let workspace = workspace.as_ref();
    let mut tools = ToolRegistry::with_policy_and_search(
        workspace,
        crate::policy::Policy::discover(workspace)?,
        crate::web::Search::discover().ok(),
    )?;
    if sandbox {
        tools.enable_sandbox(deny_network)?;
    }
    let call = ToolCall {
        id: "cli".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
        continuation: None,
    };
    let outcome = tools.execute_outcome(&call).await?;
    Ok(ToolResult {
        text: outcome.text,
        success: outcome.success,
    })
}

fn normalize(call: &ToolCall) -> Result<Cow<'_, ToolCall>> {
    let Some((_, name, operation)) = catalog::RETIRED
        .iter()
        .find(|(retired, _, _)| *retired == call.function.name)
    else {
        return Ok(Cow::Borrowed(call));
    };
    let mut call = call.clone();
    if let Some(operation) = operation {
        let mut args = parse_args(&call.function.name, &call.function.arguments)?;
        args.as_object_mut()
            .ok_or_else(|| {
                Error::Tool(format!(
                    "{} arguments must be an object",
                    call.function.name
                ))
            })?
            .insert("op".to_string(), Value::String((*operation).to_string()));
        call.function.arguments = args.to_string();
    }
    call.function.name = (*name).to_string();
    Ok(Cow::Owned(call))
}

pub fn attach(workspace: &Path, line: &str) -> Result<UserInput> {
    workspace::attach(workspace, line)
}

pub(crate) fn image_type(bytes: &[u8]) -> Option<&'static str> {
    workspace::image_type(bytes)
}

pub(crate) const IMAGES_MAX_BYTES: usize = workspace::IMAGES_MAX_BYTES;

pub(crate) fn digest(command: &str, raw: &str) -> String {
    command::digest::digest(command, raw)
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
