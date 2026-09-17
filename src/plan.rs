use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::durable::store;
use crate::provider::Provider;
use crate::runtime::Loop;
use crate::workflow::Role;

#[must_use]
pub(crate) fn file(workspace: &Path) -> PathBuf {
    workspace.join(".mage").join("plan.md")
}

pub(crate) fn write(workspace: &Path, markdown: &str) -> io::Result<PathBuf> {
    let path = file(workspace);
    store::write(&path, markdown)?;
    Ok(path)
}

#[must_use]
pub(crate) fn read(workspace: &Path) -> Option<String> {
    fs::read_to_string(file(workspace))
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
pub(crate) fn note(plan: &str) -> String {
    format!("Plan written before execution:\n{}", plan.trim())
}
