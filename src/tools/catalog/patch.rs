use crate::provider::ToolDefinition;

pub(crate) fn shaped(definitions: Vec<ToolDefinition>, patch: bool) -> Vec<ToolDefinition> {
    if !patch {
        return definitions;
    }
    let mut shaped = definitions;
    shaped.push(super::apply_patch());
    shaped
}

#[cfg(test)]
#[path = "patch_tests.rs"]
mod tests;
