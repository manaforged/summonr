use crate::policy::Grant;

use super::ToolRegistry;

pub(super) fn plan_write_note(read_only: bool, raw: &str) -> String {
    if read_only && raw.contains("Operation not permitted") {
        "\nplan mode runs the shell read-only. Return the plan in your reply. Switch to auto mode before building or editing files."
            .to_string()
    } else {
        String::new()
    }
}

pub(super) fn sandbox_grant(registry: &ToolRegistry, command: &str, raw: &str) -> Option<Grant> {
    let eperm = raw.contains("Operation not permitted") || raw.contains("Permission denied");
    let sandbox_denied = raw.contains("Operation not permitted");
    if eperm
        && let Some(grant) = crate::policy::outside_write(
            command,
            &registry.workspace,
            &registry.policy.write_roots(),
        )
    {
        return Some(grant);
    }
    if eperm && let Some(path) = crate::policy::implicit_write(command) {
        return Some(Grant::Write(path));
    }
    if registry.denies_network()
        && (network_text(raw) || ((eperm || weak_network_text(raw)) && network_verb(command)))
    {
        return Some(Grant::Network);
    }
    if registry.has_sandbox() && sandbox_denied {
        return Some(Grant::Command(command.to_string()));
    }
    None
}

fn network_text(raw: &str) -> bool {
    const MARKERS: [&str; 5] = [
        "Could not resolve host",
        "Network is unreachable",
        "Temporary failure in name resolution",
        "nodename nor servname",
        "curl: (6)",
    ];
    MARKERS.iter().any(|marker| raw.contains(marker))
}

fn weak_network_text(raw: &str) -> bool {
    const MARKERS: [&str; 3] = [
        "Failed to connect",
        "Could not connect",
        "Connection refused",
    ];
    MARKERS.iter().any(|marker| raw.contains(marker))
}

fn network_verb(command: &str) -> bool {
    segment_verbs(command).any(|verb| {
        crate::policy::verb_row(verb.strip_suffix(".exe").unwrap_or(&verb))
            .is_some_and(|row| row.network)
    })
}

pub(super) fn segment_verbs(command: &str) -> impl Iterator<Item = String> {
    crate::policy::split_segments(command)
        .into_iter()
        .filter_map(|segment| crate::policy::verb(&crate::policy::argv(&segment)))
}

pub(super) fn sandbox_note(grant: Option<&Grant>, registry: &ToolRegistry, raw: &str) -> String {
    match grant {
        Some(Grant::Network) => {
            "\nnote: network is off in the sandbox. /permissions net on for this session, or /permissions net once, then retry.\n"
                .to_string()
        }
        Some(Grant::Write(path) | Grant::Guarded(path)) => format!(
            "\nnote: {}\n",
            crate::policy::write_denied(&path.display().to_string())
        ),
        Some(Grant::Command(_)) => {
            "\nnote: the OS sandbox denied this command. Approving runs it without the sandbox.\n"
                .to_string()
        }
        Some(Grant::Danger { .. } | Grant::Edit(_)) => String::new(),
        None if registry.has_sandbox()
            && (raw.contains("Operation not permitted") || raw.contains("Permission denied")) =>
        {
            if registry.denies_network() {
                "\nnote: the OS sandbox denied this command (network is off, or the write is outside the writable roots).\n"
                    .to_string()
            } else {
                "\nnote: the OS sandbox denied a write outside the writable roots; add the directory to [sandbox] write in ~/.summonr/config.toml.\n"
                    .to_string()
            }
        }
        None => String::new(),
    }
}
