//! Scout: a read-only pass before a job's first turn. The brief names the
//! files, invariants, helpers, and risks the objective touches; it never
//! plans or edits.

use crate::provider::Provider;
use crate::runtime::Loop;
use crate::workflow::Role;

const SYSTEM: &str = "You scout a codebase before work begins. The tools are read-only. \
Report what the objective touches: the files and functions, the invariants and conventions \
they follow, existing helpers to reuse, and the risks. Be concrete: paths and names. Do not \
plan the change and do not propose edits.";

/// Run the scout on its seat and return the brief.
pub(crate) async fn brief<P: Provider>(
    loop_: &mut Loop<P>,
    objective: &str,
    usage: &mut crate::provider::Usage,
) -> crate::Result<String> {
    let (text, scout_usage) = loop_
        .aside(Role::Scout, SYSTEM, &format!("Objective:\n{objective}"))
        .await?;
    usage.add(&scout_usage);
    Ok(text)
}

/// The brief as the conversation sees it.
#[must_use]
pub(crate) fn note(brief: &str) -> String {
    format!("Scout brief:\n{}", brief.trim())
}
