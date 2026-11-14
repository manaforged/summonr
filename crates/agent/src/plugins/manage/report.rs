use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::super::{HOOKS, LSP, MCP, Plugin, Source, UNSUPPORTED, USER_CONFIG, is_json};
use crate::mcp::scan;

const COMPONENTS: [&str; 5] = ["skills", "agents", HOOKS, MCP.key, LSP.key];
const MANIFEST_SKIPPED: [(&str, &str); 5] = [
    ("outputStyles", "output styles"),
    ("workflows", "workflows"),
    ("experimental", "themes, monitors, or evals"),
    ("channels", "channels"),
    ("dependencies", "plugin dependencies"),
];

pub(super) fn report(plugin: &Plugin, manifest: &Value, off: bool) -> String {
    let mut lines = vec![format!(
        "plugin\t{}\t{}",
        plugin.name,
        crate::config::tilde(&plugin.dir)
    )];
    lines.extend(describe(plugin));
    lines.extend(skipped(plugin));
    lines.extend(settings(plugin, manifest));
    if off {
        lines.push(format!(
            "off\t{} stays unloaded until you remove it from [plugins] off",
            plugin.name
        ));
    }
    lines.join("\n")
}

pub(super) fn skipped(plugin: &Plugin) -> Vec<String> {
    let mut lines = plugin.skipped_mods();
    lines.extend(
        UNSUPPORTED
            .iter()
            .filter(|(path, _)| plugin.dir.join(path).exists())
            .map(|(path, what)| format!("skipped\t{path} (summonr does not load plugin {what})")),
    );
    lines.extend(
        MANIFEST_SKIPPED
            .iter()
            .filter(|(key, _)| plugin.manifest.get(key).is_some())
            .map(|(key, what)| format!("skipped\t{key} (summonr does not load plugin {what})")),
    );
    lines.extend(declared(plugin));
    lines.extend(crate::plugins::lsp::unused(plugin));
    for agent in crate::plugins::agents::of_plugin(plugin, &plugin.dir) {
        for field in agent.unapplied() {
            lines.push(format!(
                "skipped\tagent {} {field} (summonr does not apply it)",
                agent.name
            ));
        }
    }
    lines
}

fn declared(plugin: &Plugin) -> Vec<String> {
    let mut lines = Vec::new();
    for key in COMPONENTS {
        for source in plugin.declared(key).into_iter().flatten() {
            let note = match &source {
                Source::Outside(path) => format!("{key} {path} (outside the plugin)"),
                Source::File(path) if !path.exists() => {
                    format!("{key} {} (not found)", path.display())
                }
                Source::File(path) if [MCP.key, LSP.key].contains(&key) && !is_json(path) => {
                    format!("{key} {} (summonr reads .json files only)", path.display())
                }
                Source::File(path) if key == "agents" && !path.is_file() => {
                    format!("{key} {} (agents takes .md files)", path.display())
                }
                Source::File(_) | Source::Inline(_) => continue,
            };
            lines.push(format!("skipped\t{note}"));
        }
    }
    lines
}

pub fn describe(plugin: &Plugin) -> Vec<String> {
    let mut parts = Vec::new();
    let skills = crate::skills::plugin_files(plugin).len();
    if skills > 0 {
        parts.push(format!("skills {skills}"));
    }
    push_names(&mut parts, "mcp", &servers(plugin));
    push_names(
        &mut parts,
        "commands",
        &sorted(
            crate::plugins::commands::load(plugin)
                .into_iter()
                .map(|command| command.name),
        ),
    );
    push_names(
        &mut parts,
        "agents",
        &sorted(
            crate::plugins::agents::of_plugin(plugin, &plugin.dir)
                .into_iter()
                .map(|agent| agent.name),
        ),
    );
    push_names(
        &mut parts,
        "lsp",
        &sorted(
            crate::plugins::lsp::of(plugin, &plugin.dir)
                .into_iter()
                .filter_map(|server| server.key.split_once(':').map(|(_, name)| name.to_string())),
        ),
    );
    let hooks = plugin.hooks().names();
    if !hooks.is_empty() {
        parts.push(format!("hooks {}", hooks.join(" ")));
    }
    if has_bin_file(plugin) {
        parts.push("bin".to_string());
    }
    parts
}

fn push_names(parts: &mut Vec<String>, label: &str, names: &[String]) {
    if !names.is_empty() {
        parts.push(format!("{label} {}", names.join(" ")));
    }
}

fn sorted(names: impl Iterator<Item = String>) -> Vec<String> {
    let names: BTreeSet<String> = names.collect();
    names.into_iter().collect()
}

pub(super) fn servers(plugin: &Plugin) -> Vec<String> {
    plugin
        .mcp()
        .keys()
        .map(|server| plugin.mcp_name(server))
        .collect()
}

fn has_bin_file(plugin: &Plugin) -> bool {
    plugin.bin().is_some_and(|dir| {
        summonr_core::walk::children(&dir).any(|entry| {
            entry
                .file_type()
                .is_ok_and(|kind| kind.is_file() || kind.is_symlink())
        })
    })
}

fn settings(plugin: &Plugin, manifest: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    if manifest.get("userConfig").is_some() {
        lines.push("skipped\tuserConfig (summonr does not prompt for plugin settings)".to_string());
    }
    for (server, keys) in config_needs(plugin) {
        let tokens = keys
            .iter()
            .map(|key| format!("${{user_config.{key}}}"))
            .collect::<Vec<_>>()
            .join(" ");
        let table = format!("mcp.{}", plugin.mcp_name(&server));
        lines.push(format!(
            "warning\tmcp {server} uses {tokens}; set [{table}.user_config] or env in [{table}]"
        ));
    }
    lines
}

pub(super) fn settings_servers(plugin: &Plugin) -> BTreeSet<String> {
    config_needs(plugin)
        .into_keys()
        .map(|server| plugin.mcp_name(&server))
        .collect()
}

fn config_needs(plugin: &Plugin) -> BTreeMap<String, Vec<String>> {
    plugin
        .mcp()
        .into_iter()
        .filter_map(|(server, entry)| {
            let keys: Vec<String> = config_keys(&entry.to_string()).into_iter().collect();
            (!keys.is_empty()).then_some((server, keys))
        })
        .collect()
}

fn config_keys(text: &str) -> BTreeSet<String> {
    let keys = std::cell::RefCell::new(BTreeSet::new());
    scan(text, USER_CONFIG, |key| {
        if summonr_core::text::is_name(key, "_-.") {
            keys.borrow_mut().insert(key.to_string());
        }
        None
    });
    keys.into_inner()
}
