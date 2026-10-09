#![recursion_limit = "256"]

pub mod acp;
pub mod askpass;
pub mod catalog;
mod checkpoint;
pub mod child;
mod command;
mod config;
mod credentials;
pub mod diff;
pub(crate) mod durable;

pub(crate) const SKIP_DIRS: &[&str] = &[
    ".git",
    ".summonr",
    "target",
    "node_modules",
    "dist",
    "__pycache__",
];
pub mod classify;
mod endpoint;
mod error;
mod goal;
mod hashline;
mod herdr;
mod hooks;
mod hunks;
mod jobs;
mod language;
pub mod limits;
mod mcp;
pub use mcp::{
    Board as McpBoard, Choice as McpChoice, Field as McpField, FieldKind as McpFieldKind,
    Form as McpForm, FormAction as McpFormAction, FormAnswer as McpFormAnswer, McpLaunch, McpState,
    McpTransport, ServerStatus, TextFormat as McpTextFormat, add as mcp_add,
    call_label as mcp_call_label, probe as mcp_probe, remove as mcp_remove, status as mcp_status,
};
pub mod modules;
mod plan;
mod plugins;
mod policy;
mod prompt;
mod provider;
mod recent;
mod review;
mod rpc;
mod runtime;
mod scout;
pub mod secret;
mod session;
mod shell;
mod skills;
pub mod supervise;
mod text;
mod tools;
mod verify;
mod web;
mod worktree;

pub fn stop_children() {
    tools::command::stop_groups();
}

pub mod mail;
pub mod prefs;
pub mod splash;
pub mod steps;
pub mod tui;
pub mod workflow;

pub use command::{SlashCommand, command_specs, parse_command};
pub use config::tilde;
pub use config::{check as check_config, sandbox_on};
pub use durable::store::lock as lock_file;
pub use durable::work::{WorkKind, WorkProgress};
pub use endpoint::{
    AuthInfo, Endpoint, LoginFlow, Need, Pick, State as EndpointState, add_server as add_endpoint,
    adopt_keys, auth_info, check_key, chosen_by as endpoint_chosen_by, closest as closest_endpoint,
    fallbacks as endpoint_fallbacks, first_classify as classify_endpoint, is_named as is_endpoint,
    label as endpoint_label, login_seat, logout, names as endpoint_names,
    ordered_names as endpoint_names_ordered, remove_server as remove_endpoint,
    resolve as resolve_endpoint, set_up as endpoint_set_up, vision as vision_endpoint,
};
pub use error::{Error, ProviderFailureKind, Result};
pub use goal::prompt as goal_prompt;
pub use jobs::live as live_jobs;
pub use plugins::{install as install_plugin, list as list_plugins, uninstall as uninstall_plugin};
pub use prompt::{Sources, summonr_home, system_prompt};
pub(crate) use provider::dollars;
pub use provider::{Client, Effort, Usage, UserInput};
pub use recent::default_effort;
pub use runtime::{Budget, Loop, RunMode, StepRun, Turn};
pub use session::{
    ProfileScope, export as export_session, last_for, list as list_sessions, profile,
    prune as prune_sessions, resolve_id as resolve_session, short,
};
pub use tools::sandbox::Network;
#[cfg(all(windows, feature = "windows-sandbox"))]
pub use tools::sandbox::windows::entry as sandbox_entry;
pub use tools::{AbortCode, Event, ToolResult, attach, run_tool};
pub use verify::{Receipt, check_commands};
pub use worktree::{
    Tree, add as add_worktree, finish as finish_worktree, in_git, list as list_worktrees,
    status as worktree_status, sweep as sweep_worktrees,
};

#[must_use]
pub fn revision() -> &'static str {
    option_env!("SUMMONR_REVISION").unwrap_or("unknown")
}
