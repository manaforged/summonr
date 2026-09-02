mod child;
mod command;
mod config;
pub mod diff;
pub(crate) mod durable;

pub(crate) const SKIP_DIRS: &[&str] = &[
    ".git",
    ".mage",
    "target",
    "node_modules",
    "dist",
    "__pycache__",
];
mod endpoint;
mod error;
mod goal;
mod hashline;
mod herdr;
mod jobs;
mod language;
mod policy;
mod prompt;
mod provider;
mod recent;
mod review;
mod runtime;
mod scout;
pub mod secret;
mod session;
mod skills;
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

pub use command::{CommandSpec, SlashCommand, command_specs, parse_command};
pub use durable::work::{WorkKind, WorkPhase, WorkProgress};
pub use endpoint::{
    AuthInfo, Endpoint, adopt_keys, auth_info, forget_key, is_named as is_endpoint,
    names as endpoint_names, resolve as resolve_endpoint,
    resolve_unauthed as resolve_endpoint_unauthed, save_key,
};
pub use error::{Error, ProviderFailure, ProviderFailureKind, Result};
pub use goal::{DEFAULT_MAX_TURNS, prompt as goal_prompt};
pub use hashline::{line_hash, tagged_text};
pub use jobs::live as live_jobs;
pub use prompt::{harness_section, mage_home, system_prompt, system_prompt_with};
pub use provider::{Client, Effort, Image, Message, Price, Provider, Usage, UserInput, dollars};
pub use runtime::{Budget, Loop, Turn};
pub use session::{
    Replay, SessionInfo, export as export_session, last_for, list as list_sessions,
    resolve_id as resolve_session, set_name as name_session, short as short_session,
};
pub use tools::{Event, attach};
pub use verify::{Outcome, Receipt, check_commands};
pub use worktree::{
    Status as WorktreeStatus, Tree, add as add_worktree, finish as finish_worktree, in_git,
    list as list_worktrees, status as worktree_status, sweep as sweep_worktrees,
};

#[must_use]
pub fn revision() -> &'static str {
    option_env!("SUMMON_REVISION").unwrap_or("unknown")
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
