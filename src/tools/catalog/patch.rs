use crate::provider::ToolDefinition;

pub(crate) fn shaped(definitions: Vec<ToolDefinition>, patch: bool) -> Vec<ToolDefinition> {
    if !patch {
        return definitions;
    }
    let mut shaped: Vec<ToolDefinition> = definitions
        .into_iter()
        .filter(|tool| {
            !super::Builtin::live(&tool.function.name).is_some_and(super::Builtin::edits)
        })
        .collect();
    shaped.push(super::apply_patch());
    shaped
}

#[cfg(test)]
#[path = "patch_tests.rs"]
mod tests;
