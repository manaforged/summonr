use super::*;

pub(super) fn rebase_paths(text: &str, from: &Path, to: &Path) -> String {
    let from = from.to_string_lossy();
    let to = to.to_string_lossy();
    if from.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(from.as_ref()) {
        let end = at + from.len();
        let bounded = rest[end..].chars().next().is_none_or(|next| {
            next == '/' || next == '\\' || !(next.is_alphanumeric() || matches!(next, '-' | '_'))
        });
        out.push_str(&rest[..at]);
        out.push_str(if bounded { to.as_ref() } else { from.as_ref() });
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

pub(super) fn instruction(access: Access, isolated: bool) -> &'static str {
    match (access, isolated) {
        (Access::Implement, true) => {
            "You are in an isolated checkout of the parent workspace. It shares the parent build cache. Do not commit. Do not delete target or node_modules. The parent applies your edits when you finish, even if another child fails. Background commands stay with the parent."
        }
        (Access::Implement, false) => {
            "Edit this workspace. You may build, test, and lint here. Background commands stay with the parent."
        }
        (Access::Research, _) => "This assignment is read-only. Do not change the workspace.",
    }
}

pub(super) fn agent_brief(
    name: Option<&str>,
    workspace: &Path,
) -> Result<(String, Option<Vec<String>>)> {
    let Some(name) = name else {
        return Ok((String::new(), None));
    };
    let agent = crate::plugins::agents::find(name, workspace).map_err(Error::Tool)?;
    if let Some(tools) = &agent.tools
        && !tools
            .iter()
            .any(|tool| crate::hooks::builtin(tool).is_some())
    {
        return Err(Error::Tool(format!(
            "agent `{name}` declares tools [{}] but none is a known tool",
            tools.join(", ")
        )));
    }
    Ok((format!("{}\n\n", agent.instructions), agent.tools))
}
