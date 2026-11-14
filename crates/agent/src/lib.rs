#![recursion_limit = "256"]

/// The product id: binary, client and author name.
pub const PRODUCT: &str = "summonr";

pub mod acp;
pub mod askpass;
pub mod catalog;
mod checkpoint;
pub mod child;
pub mod command;
mod config;
pub mod credentials;
pub mod diff;
pub mod display;
pub mod durable;
pub mod forge;

pub mod classify;
pub mod endpoint;
mod goal;
mod hashline;
pub mod herdr;
mod hooks;
pub mod hunks;
pub mod jobs;
mod language;
pub mod limits;
pub mod mcp;
pub use mcp::{
    Board as McpBoard, Choice as McpChoice, Field as McpField, FieldKind as McpFieldKind,
    Form as McpForm, FormAction as McpFormAction, FormAnswer as McpFormAnswer, McpLaunch, McpState,
    McpTransport, ServerStatus, TextFormat as McpTextFormat, add as mcp_add,
    call_label as mcp_call_label, probe as mcp_probe, remove as mcp_remove, status as mcp_status,
};
pub mod browser;
pub mod memory;
pub mod modules;
pub mod panel;
pub mod plugins;
pub mod policy;
pub mod process;
mod prompt;
pub mod provider;
pub mod quota;
pub mod recent;
mod rpc;
pub mod runtime;
mod scout;
pub mod scripts;
pub mod secret;
pub mod session;
pub mod skills;
pub mod supervise;
pub mod tools;
mod verify;
mod web;
pub mod worktree;

pub fn stop_children() {
    tools::command::stop_groups();
    tools::command::stop_child_jobs();
}

#[cfg(unix)]
pub async fn interrupted() -> i32 {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut interrupt), Ok(mut terminate), Ok(mut hangup)) = (
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
        signal(SignalKind::hangup()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = interrupt.recv() => 130,
        _ = terminate.recv() => 143,
        _ = hangup.recv() => 129,
    }
}

#[cfg(not(unix))]
pub async fn interrupted() -> i32 {
    if tokio::signal::ctrl_c().await.is_ok() {
        return 130;
    }
    std::future::pending().await
}

pub mod cron;
pub mod gateway;
pub mod hub;
pub mod mail;
pub mod prefs;
pub mod splash;
pub mod steps;
pub mod workflow;

pub use command::{SlashCommand, command_specs, parse_command, table};
pub use config::check as check_config;
pub use config::tilde;
pub use durable::store::lock as lock_file;
pub use durable::work::{WorkKind, WorkProgress};
pub use endpoint::{
    AuthInfo, Endpoint, LoginFlow, Near as EndpointNear, Origin, Requirement,
    State as EndpointState, add_server as add_endpoint, adopt_keys, auth_info, check_key,
    chosen_by as endpoint_chosen_by, closest as closest_endpoint, fallbacks as endpoint_fallbacks,
    first_classify as classify_endpoint, is_named as is_endpoint, login_endpoint, logout,
    names as endpoint_names, ordered_names as endpoint_names_ordered,
    remove_server as remove_endpoint, resolve as resolve_endpoint,
    set_default as set_default_endpoint, set_up as endpoint_set_up, vision as vision_endpoint,
};
pub use goal::prompt as goal_prompt;
pub use jobs::live as live_jobs;
pub use plugins::manage::Plugin;
pub use prompt::{Sources, system_prompt};
pub use provider::dollars;
pub use provider::{Client, Delta, Effort, Usage, UserInput};
pub use runtime::{Budget, Job, Loop, NextAction, Turn};
pub use session::{
    ProfileScope, export as export_session, export_json, info as session_info, last_for,
    list as list_sessions, mark_interactive, profile, prune as prune_sessions,
    resolve_id as resolve_session, short,
};
pub use summonr_core::dirs::summonr_home;
pub use summonr_core::error::{Error, ProviderFailureKind, Result};

pub const INDEX_ENTRY: &str = tools::INDEX_ENTRY;

pub fn index_files(root: &std::path::Path) -> i32 {
    i32::from(tools::build_file_index(root).is_err())
}
pub use tools::{AbortCode, Event, StepState, ToolResult, attach, run_tool};
pub use verify::{Receipt, check_commands};
pub use worktree::{
    Tree, add as add_worktree, finish as finish_worktree, in_git, list as list_worktrees,
    status as worktree_status, sweep as sweep_worktrees, tree_at as worktree_at,
};

#[must_use]
pub fn revision() -> &'static str {
    option_env!("SUMMONR_REVISION").unwrap_or("unknown")
}

#[cfg(test)]
#[path = "fuzz_tests.rs"]
pub(crate) mod fuzz_tests;

#[cfg(all(test, unix))]
#[path = "fixture_tests.rs"]
pub(crate) mod fixture_tests;
