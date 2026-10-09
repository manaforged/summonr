use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::durable::store;
use crate::provider::Provider;
use crate::runtime::Loop;
use crate::workflow::Role;

const FILE: &str = "plan.md";

const APPROVALS: &[&str] = &[
    "go",
    "go ahead",
    "do it",
    "yes",
    "y",
    "ok",
    "okay",
    "implement it",
    "implement",
    "proceed",
    "ship it",
    "accept",
    "approved",
    "lgtm",
    "sounds good",
];

#[must_use]
pub(crate) fn file(session: &Path) -> PathBuf {
    session.with_file_name(FILE)
}

pub(crate) fn write(session: &Path, markdown: &str) -> io::Result<PathBuf> {
    let path = file(session);
    store::write(&path, markdown)?;
    Ok(path)
}

#[must_use]
pub(crate) fn read(session: &Path) -> Option<String> {
    fs::read_to_string(file(session))
        .ok()
        .filter(|text| !text.trim().is_empty())
}

const SYSTEM: &str = "You write the plan before the work starts. The tools are read-only. \
Inspect only what the objective touches, then answer with a plan and nothing else: one line of \
scope, then a `## Steps` section whose numbered items are the work in order. Each step is one \
action, names the file or symbol it touches, and ends with a check on a `Run `command`` line when \
a command proves it. Do not do the work and do not propose the diff.";

pub(crate) async fn draft<P: Provider>(
    loop_: &mut Loop<P>,
    objective: &str,
    usage: &mut crate::provider::Usage,
) -> crate::Result<String> {
    let (text, plan_usage) = loop_
        .aside(Role::Plan, SYSTEM, &format!("Objective:\n{objective}"))
        .await?;
    usage.add(&plan_usage);
    Ok(text)
}

#[must_use]
pub(crate) fn approves(reply: &str) -> bool {
    let reply = reply.trim().to_lowercase();
    APPROVALS.contains(&reply.as_str())
}

#[must_use]
pub(crate) fn ready(session: &Path) -> String {
    format!(
        "plan saved to {}; /plan accept runs it, /plan reject drops it",
        file(session).display()
    )
}

#[must_use]
pub(crate) fn note(plan: &str) -> String {
    format!("Plan written before execution:\n{}", plan.trim())
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
