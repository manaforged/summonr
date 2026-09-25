#![recursion_limit = "256"]

pub mod acp;
pub mod askpass;
mod bundle;
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
mod jobs;
mod language;
pub mod limits;
mod mcp;
pub mod modules;
mod plan;
mod policy;
mod prompt;
mod provider;
mod recent;
mod review;
pub mod room;
mod rpc;
mod runtime;
mod scout;
pub mod secret;
mod session;
mod shell;
mod skills;
mod text;
mod tools;
mod verify;
mod web;
mod worktree;

pub(crate) mod bus;
pub mod prefs;
pub mod splash;
pub mod steps;
pub mod tui;
pub mod workflow;

pub use bundle::{add as add_bundle, list as list_extras};
pub use command::{CommandSpec, SlashCommand, command_specs, parse_command};
pub use config::tilde;
pub use config::{check as check_config, sandbox_on};
pub use durable::store::lock as lock_file;
pub use durable::work::{WorkKind, WorkPhase, WorkProgress};
pub use endpoint::{
    AuthInfo, Endpoint, LoginFlow, State as EndpointState, adopt_keys, auth_info, check_key,
    chosen_by as endpoint_chosen_by, closest as closest_endpoint, fallbacks as endpoint_fallbacks,
    first_classify as classify_endpoint, is_named as is_endpoint, logout, names as endpoint_names,
    resolve as resolve_endpoint, resolve_unauthed as resolve_endpoint_unauthed,
    vision as vision_endpoint,
};
pub use error::{Error, ProviderFailure, ProviderFailureKind, Result};
pub use goal::prompt as goal_prompt;
pub use hashline::{line_hash, tagged_text};
pub use jobs::live as live_jobs;
pub use prompt::{harness_section, summonr_home, system_prompt, system_prompt_with};
pub use provider::{
    Client, Effort, Image, Message, Partial, Price, Provider, Usage, UserInput, dollars,
};
pub use recent::Seat;
pub use runtime::{Budget, Loop, RunMode, StepRun, Turn};
pub use session::{
    Replay, SessionInfo, Spend, export as export_session, last_for, list as list_sessions,
    profile as profile_session, profile_all as profile_sessions, resolve_id as resolve_session,
    set_name as name_session, short,
};
#[cfg(all(windows, feature = "windows-sandbox"))]
pub use tools::sandbox::windows::entry as sandbox_entry;
pub use tools::{Event, ToolResult, attach, run_tool};
pub use verify::{Outcome, Receipt, check_commands};
pub use worktree::{
    Status as WorktreeStatus, Tree, add as add_worktree, finish as finish_worktree, in_git,
    list as list_worktrees, status as worktree_status, sweep as sweep_worktrees,
};

#[must_use]
pub fn revision() -> &'static str {
    option_env!("SUMMONR_REVISION").unwrap_or("unknown")
}
