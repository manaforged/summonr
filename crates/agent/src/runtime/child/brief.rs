use super::*;
use crate::plugins::agents::{Agent, Model};

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
            "Edit only your assigned paths in this shared workspace; other lanes may be writing here. Edits are immediately visible and are not reconciled or rolled back by the harness. Do not commit. Background commands stay with the parent."
        }
        (Access::Research, _) => {
            "This assignment is research: investigate and report what you find. Change the workspace only where the assignment asks. Do not commit."
        }
    }
}

pub(super) fn agent_of(name: Option<&str>, workspace: &Path) -> Result<Option<Agent>> {
    let Some(name) = name else {
        return Ok(None);
    };
    let agent = crate::plugins::agents::find(name, workspace).map_err(Error::Tool)?;
    if !agent.scope().resolves() {
        return Err(Error::Tool(format!(
            "agent `{name}` declares tools [{}] but none is a tool it may use",
            agent.tools.as_deref().unwrap_or_default().join(", ")
        )));
    }
    Ok(Some(agent))
}

fn claude_endpoint(
    name: Option<&str>,
    current: &str,
    model: &Model,
) -> std::result::Result<crate::endpoint::Endpoint, String> {
    use crate::provider::{Vendor, traits};
    let name = name.ok_or("the parent provider has no configured endpoint")?;
    if traits(current).vendor != Vendor::Anthropic {
        return Err(format!(
            "endpoint `{name}` runs `{current}`, not a Claude model"
        ));
    }
    let need = crate::endpoint::Requirement::ModelAndKey;
    let endpoint = crate::endpoint::resolve(Some(name), Some(model.text()), need)
        .map_err(|error| error.to_string())?;
    if traits(&endpoint.model).vendor != Vendor::Anthropic {
        return Err(format!("endpoint `{name}` resolves it to no Claude model"));
    }
    Ok(endpoint)
}

impl<P: Provider> Loop<P> {
    pub(super) fn agent_endpoint(
        &self,
        args: &Args,
        agent: Option<&Agent>,
        notes: &mut Vec<String>,
    ) -> Result<Option<crate::endpoint::Endpoint>> {
        let endpoint = args.endpoint()?;
        let Some(model) = agent.and_then(|agent| agent.model.as_ref()) else {
            return Ok(endpoint);
        };
        if endpoint.is_some() || *model == Model::Inherit {
            return Ok(endpoint);
        }
        let (provider, current) = self.selection(crate::workflow::Role::Implement);
        Ok(
            match claude_endpoint(provider.endpoint_name(), current, model) {
                Ok(found) => Some(found),
                Err(reason) => {
                    notes.push(format!(
                        "agent model `{}` ignored ({reason}); the child inherits `{current}`",
                        model.text()
                    ));
                    None
                }
            },
        )
    }

    pub(super) fn child_opener(
        &self,
        args: &Args,
        agent: Option<&Agent>,
        tree: Option<&Tree>,
        workspace: &Path,
    ) -> String {
        let instruction = instruction(args.access, tree.is_some());
        let brief = agent.map_or_else(String::new, |agent| format!("{}\n\n", agent.instructions));
        let assignment = tree.map_or_else(
            || args.assignment.clone(),
            |tree| rebase_paths(&args.assignment, &self.workspace, &tree.path),
        );
        let stay = if args.name.is_some() {
            " Finish the assigned task, then stay. Mail to your name is the next turn."
        } else {
            " Finish only the assigned task."
        };
        format!(
            "Harness: child.{stay} {instruction} Your working directory is {}.\n\n{brief}{assignment}",
            workspace.display()
        )
    }

    pub(super) fn child_tools(
        &self,
        agent: Option<&Agent>,
        workspace: &Path,
        child: &Self,
    ) -> Result<crate::tools::ToolRegistry> {
        let mut registry = self.tools.child_at(workspace, &child.session_id())?;
        if self.tools.is_read_only() {
            registry.contain_read_only();
        }
        if let Some(agent) = agent {
            registry.set_scope(agent.scope());
        }
        Ok(registry)
    }
}
