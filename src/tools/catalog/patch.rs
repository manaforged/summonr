use crate::provider::ToolDefinition;
use crate::tools::tool;

pub(crate) fn shaped(definitions: Vec<ToolDefinition>, patch: bool) -> Vec<ToolDefinition> {
    if !patch {
        return definitions;
    }
    let mut shaped: Vec<ToolDefinition> = definitions
        .into_iter()
        .filter(|tool| !matches!(tool.function.name.as_str(), "edit" | "write"))
        .collect();
    shaped.push(apply_patch());
    shaped
}

fn apply_patch() -> ToolDefinition {
    tool(
        "apply_patch",
        "Create, change, or delete files with one patch. The patch text is:\n*** Begin Patch\n*** Update File: path/to/file\n@@ optional line that sits above the change\n context line\n-removed line\n+added line\n*** Add File: path/to/new\n+every line of the new file\n*** Delete File: path/to/old\n*** End Patch\nUpdate hunks need about three unchanged context lines around each change; one patch can touch several files, and nothing is written unless every section applies.",
        serde_json::json!({
            "type": "object",
            "properties": {"patch": {"type": "string"}},
            "required": ["patch"],
            "additionalProperties": false
        }),
    )
}

#[cfg(test)]
#[path = "patch_tests.rs"]
mod tests;
