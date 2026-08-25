//! Summoner is a small agent harness with an OpenAI-compatible provider boundary.
//!
//! Workspace tools, hashline edits, policy, and an append-only session log.

mod command;
mod config;
pub(crate) mod durable;
mod endpoint;
mod error;
mod goal;
mod hashline;
mod jobs;
mod policy;
mod prompt;
mod provider;
mod runtime;
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

pub use command::{CommandSpec, SlashCommand, command_specs, parse_command};
pub use durable::work::{WorkKind, WorkPhase, WorkProgress};
pub use endpoint::{
    Endpoint, is_named as is_endpoint, names as endpoint_names, resolve as resolve_endpoint,
};
pub use error::{Error, ProviderFailure, ProviderFailureKind, Result};
pub use goal::{DEFAULT_MAX_TURNS, prompt as goal_prompt};
pub use hashline::{line_hash, tagged_text};
pub use prompt::{harness_section, mage_home, system_prompt, system_prompt_with};
pub use provider::{Client, Image, Message, Provider, Usage, UserInput};
pub use runtime::{Budget, Loop, Turn};
pub use session::{
    Replay, SessionInfo, last_for, list as list_sessions, resolve_id as resolve_session,
};
pub use tools::{Event, attach};
pub use verify::{Outcome, check_commands};
pub use worktree::{
    Tree, add as add_worktree, finish as finish_worktree, in_git, list as list_worktrees,
    sweep as sweep_worktrees,
};

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
