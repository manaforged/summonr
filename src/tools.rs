use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::policy::{Grant, Policy};
use crate::provider::{ToolCall, ToolFunction, ToolKind, UserInput};
use crate::web::Search;

mod args;
mod budget;
mod event;
pub(crate) use event::progress_text;
pub(crate) use event::{Abort, Hold, Kept};
pub use event::{AbortCode, Event};
mod catalog;
mod dedup;
pub(crate) use catalog::{Builtin, readonly, retired_op, shaped};
pub(crate) use dedup::read_span;
pub(crate) mod command;
mod lsp;
mod paths;
mod search;
mod spill;
mod todo;
mod workspace;

use args::{nonempty_path, object_bool, object_string, object_usize};
pub(crate) use args::{parse_args, tool};
use todo::parse_todo;
pub(crate) use todo::{Todo, TodoStatus, todos_from_args};
pub(crate) use workspace::patch_files;
pub(crate) use workspace::search::index::{ENTRY as INDEX_ENTRY, build as build_file_index};

pub(crate) fn blocking<T>(work: impl FnOnce() -> T) -> T {
    let flavor = tokio::runtime::Handle::try_current().map(|handle| handle.runtime_flavor());
    if matches!(flavor, Ok(tokio::runtime::RuntimeFlavor::MultiThread)) {
        tokio::task::block_in_place(work)
    } else {
        work()
    }
}

pub(crate) fn command_exit_code(output: &str) -> Option<i32> {
    command::exit_code(output)
}

pub(crate) struct ToolRegistry {
    workspace: PathBuf,
    policy: Policy,
    search: Option<Search>,
    browser: Option<crate::browser::Browser>,
    env: Vec<(String, String)>,
    background: bool,
    language: bool,
    skills: bool,
    probed: std::sync::OnceLock<(bool, bool)>,
    lsp: std::sync::Mutex<
        std::collections::BTreeMap<String, std::sync::Arc<tokio::sync::Mutex<Option<lsp::Conn>>>>,
    >,
    session: std::sync::Mutex<Option<String>>,
    read_ranges: std::sync::Mutex<dedup::Ranges>,
    spill_dir: std::sync::OnceLock<tempfile::TempDir>,
    launches: lsp::Launches,
    roots: Vec<paths::Root>,
    approved_once: std::sync::Mutex<Option<command::Approval>>,
    wrote: std::sync::Mutex<std::collections::BTreeSet<String>>,
    model_images: bool,
    vision: Option<crate::endpoint::Endpoint>,
    only: Option<Vec<Builtin>>,
    standalone: bool,
    task_list: bool,
}

pub(crate) struct ToolOutput {
    pub(crate) text: String,
    pub(crate) success: bool,
    pub(crate) mutated: bool,
    pub(crate) grant: Option<Grant>,
    pub(crate) images: Vec<crate::provider::Image>,
    pub(crate) patch: Option<String>,
}

impl ToolOutput {
    pub(crate) fn ok(text: String) -> Self {
        Self {
            text,
            success: true,
            mutated: false,
            grant: None,
            images: Vec::new(),
            patch: None,
        }
    }

    pub(crate) fn mutated(text: String, patch: Option<String>) -> Self {
        Self {
            mutated: true,
            patch,
            ..Self::ok(text)
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
            roots: paths::roots(&policy),
            workspace,
            policy,
            search,
            browser: None,
            env: Vec::new(),
            background: true,
            language: true,
            skills: true,
            probed: std::sync::OnceLock::new(),
            lsp: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            session: std::sync::Mutex::new(None),
            read_ranges: std::sync::Mutex::new(std::collections::HashMap::new()),
            spill_dir: std::sync::OnceLock::new(),
            launches: lsp::Launches::default(),
            approved_once: std::sync::Mutex::new(None),
            wrote: std::sync::Mutex::new(std::collections::BTreeSet::new()),
            model_images: false,
            vision: None,
            only: None,
            standalone: false,
            task_list: true,
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

    pub(crate) fn set_model_images(&mut self, model_images: bool) {
        self.model_images = model_images;
    }

    pub(crate) fn set_strict(&mut self) {
        self.policy.set_strict();
    }

    pub(crate) fn toggle_approval(&mut self) -> bool {
        self.policy.toggle_approval()
    }

    pub(crate) fn set_unrestricted(&mut self) {
        self.policy.set_unrestricted();
    }

    pub(crate) fn child_at(&self, workspace: &Path, session: &str) -> Result<Self> {
        let mut child = Self::with_policy_and_search(workspace, self.policy.clone(), None)?;
        child.env.clone_from(&self.env);
        child.background = false;
        child.language = self.language;
        child.skills = self.skills;
        child.search.clone_from(&self.search);
        child.browser.clone_from(&self.browser);
        child.set_session(session);
        Ok(child)
    }

    pub(crate) fn approve_once(&self, approval: command::Approval) -> Result<()> {
        *self
            .approved_once
            .lock()
            .map_err(|error| Error::Tool(format!("cannot record command approval: {error}")))? =
            Some(approval);
        Ok(())
    }

    pub(crate) fn take_approval(&self) -> Result<Option<command::Approval>> {
        Ok(self
            .approved_once
            .lock()
            .map_err(|error| Error::Tool(format!("cannot consume command approval: {error}")))?
            .take())
    }

    pub(crate) fn is_strict(&self) -> bool {
        self.policy.strict
    }

    pub(crate) fn policy_inert(&self) -> bool {
        self.policy.is_unrestricted() && self.policy.configured()
    }

    pub(crate) fn is_read_only(&self) -> bool {
        self.policy.is_read_only()
    }

    pub(crate) fn policy(&self) -> &Policy {
        &self.policy
    }

    pub(crate) fn set_session(&self, id: &str) {
        if let Ok(mut session) = self.session.lock() {
            *session = Some(id.to_string());
        }
    }

    pub(crate) fn set_task_list(&mut self, on: bool) {
        self.task_list = on;
    }

    pub(crate) fn set_read_only(&mut self, on: bool) {
        self.policy.set_read_only(on);
    }

    pub(crate) fn contain_read_only(&mut self) {
        self.policy.set_read_only(true);
        self.browser = None;
    }

    pub(crate) fn restrict_to(&mut self, names: &[String]) {
        self.only = Some(
            names
                .iter()
                .filter_map(|name| crate::hooks::builtin(name))
                .collect(),
        );
    }

    pub(crate) fn clear_wrote(&self) {
        if let Ok(mut wrote) = self.wrote.lock() {
            wrote.clear();
        }
    }

    pub(crate) fn note_wrote(&self, path: &str) {
        let path = path.trim().trim_start_matches("./").replace('\\', "/");
        if path.is_empty() {
            return;
        }
        if let Ok(mut wrote) = self.wrote.lock() {
            wrote.insert(path);
        }
    }

    pub(crate) fn wrote(&self) -> Vec<String> {
        self.wrote
            .lock()
            .map(|wrote| wrote.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) async fn execute_outcome(&self, call: &ToolCall) -> Result<ToolOutput> {
        let (name, op) = normalize(call);
        match Builtin::live(name) {
            Some(kind) if self.only.as_ref().is_some_and(|only| !only.contains(&kind)) => Err(
                Error::Denied(format!("tool `{name}` is not one of this agent's tools")),
            ),
            Some(kind) if self.enabled(kind) => {
                self.fit_output(self.dispatch(kind, name, op, call).await)
            }
            Some(kind) if kind != Builtin::Task => Err(Error::Denied(format!(
                "tool `{name}` is unavailable in the current mode"
            ))),
            Some(_) | None => Err(Error::Tool(format!("unknown tool `{name}`"))),
        }
    }

    async fn dispatch(
        &self,
        kind: Builtin,
        name: &str,
        op: Option<&str>,
        call: &ToolCall,
    ) -> Result<ToolOutput> {
        let args = &match kind {
            Builtin::Browser => {
                return self
                    .browse(&call.function.arguments)
                    .await
                    .map(ToolOutput::ok);
            }
            Builtin::Task => return Err(Error::Tool(format!("unknown tool `{name}`"))),
            _ => arguments_of(name, op, call)?,
        };
        match kind {
            Builtin::Read => blocking(|| self.read(args)).map(|out| self.dedup_read(args, out)),
            Builtin::Search => blocking(|| self.search_files(args)).map(ToolOutput::ok),
            Builtin::Edit => self.edit_file(args).await,
            Builtin::Write => self.write_file(args).await,
            Builtin::Skill => self.skill(args).map(ToolOutput::ok),
            Builtin::RunCommand => self.run_command(args).await,
            Builtin::Todo => parse_todo(args).map(ToolOutput::ok),
            Builtin::Web => self.web(args).await.map(ToolOutput::ok),
            Builtin::Lsp => self.lsp(args).await,
            Builtin::Browser | Builtin::Task => Err(Error::Tool("unknown tool".to_string())),
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
) -> Result<ToolResult> {
    let workspace = workspace.as_ref();
    let mut tools = ToolRegistry::with_policy_and_search(
        workspace,
        crate::policy::Policy::discover(workspace)?,
        None,
    )?;
    tools.standalone = true;
    tools.set_modules(&crate::modules::Switches::load(workspace)?)?;
    let call = ToolCall {
        id: "cli".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
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

fn normalize(call: &ToolCall) -> (&str, Option<&'static str>) {
    catalog::retired(&call.function.name)
        .map_or((call.function.name.as_str(), None), |(_, live, op)| {
            (live.name(), *op)
        })
}

fn arguments_of(name: &str, op: Option<&str>, call: &ToolCall) -> Result<Value> {
    let Some(op) = op else {
        return parse_args(name, &call.function.arguments);
    };
    let mut args = parse_args(&call.function.name, &call.function.arguments)?;
    args.as_object_mut()
        .ok_or_else(|| {
            Error::Tool(format!(
                "{} arguments must be an object",
                call.function.name
            ))
        })?
        .insert("op".to_string(), Value::String(op.to_string()));
    Ok(args)
}

pub fn attach(workspace: &Path, line: &str) -> Result<UserInput> {
    workspace::attach(workspace, line)
}

pub(crate) fn image_type(bytes: &[u8]) -> Option<&'static str> {
    workspace::image_type(bytes)
}

pub(crate) const IMAGES_MAX_BYTES: usize = workspace::IMAGES_MAX_BYTES;

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
