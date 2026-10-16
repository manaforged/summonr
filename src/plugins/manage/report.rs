use std::collections::BTreeSet;

use serde_json::Value;

use super::super::{Plugin, UNSUPPORTED, USER_CONFIG};

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
    lines
}

pub(super) fn describe(plugin: &Plugin) -> Vec<String> {
    let mut parts = Vec::new();
    let skills = skill_count(plugin);
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

fn skill_count(plugin: &Plugin) -> usize {
    let mut found = BTreeSet::new();
    for dir in plugin.skill_dirs() {
        if dir.join("SKILL.md").is_file() {
            found.insert(dir.clone());
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("SKILL.md").is_file() {
                found.insert(path);
            }
        }
    }
    found.len()
}

fn sorted(names: impl Iterator<Item = String>) -> Vec<String> {
    let names: BTreeSet<String> = names.collect();
    names.into_iter().collect()
}

pub(super) fn servers(plugin: &Plugin) -> Vec<String> {
    let mut names = BTreeSet::new();
    for map in plugin.mcp_json() {
        if let Some(object) = map.as_object() {
            names.extend(object.keys().cloned());
        }
    }
    let toml_servers = plugin.mcp_toml();
    if let Some(table) = toml_servers.as_ref().and_then(toml::Value::as_table) {
        names.extend(table.keys().cloned());
    }
    names
        .into_iter()
        .map(|server| format!("{}-{server}", plugin.name))
        .collect()
}

fn has_bin_file(plugin: &Plugin) -> bool {
    plugin.bin().is_some_and(|dir| {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_type()
                    .is_ok_and(|kind| kind.is_file() || kind.is_symlink())
            })
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
        let table = format!("mcp.{}-{server}", plugin.name);
        lines.push(format!(
            "warning\tmcp {server} uses {tokens}; set [{table}.user_config] or env in [{table}]"
        ));
    }
    lines
}

pub(super) fn settings_servers(plugin: &Plugin) -> BTreeSet<String> {
    config_needs(plugin)
        .into_keys()
        .map(|server| format!("{}-{server}", plugin.name))
        .collect()
}

fn config_needs(plugin: &Plugin) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut needs = std::collections::BTreeMap::new();
    let mut note = |server: &str, text: String| {
        let keys: Vec<String> = config_keys(&text).into_iter().map(str::to_string).collect();
        if !keys.is_empty() {
            needs.entry(server.to_string()).or_insert(keys);
        }
    };
    for map in plugin.mcp_json() {
        if let Some(object) = map.as_object() {
            for (server, entry) in object {
                note(server, entry.to_string());
            }
        }
    }
    if let Some(table) = plugin.mcp_toml().as_ref().and_then(toml::Value::as_table) {
        for (server, entry) in table {
            note(server, entry.to_string());
        }
    }
    needs
}

fn config_keys(text: &str) -> BTreeSet<&str> {
    let mut keys = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find(USER_CONFIG) {
        let after = &rest[start + USER_CONFIG.len()..];
        let Some(end) = after.find('}') else {
            break;
        };
        let key = &after[..end];
        if key_ok(key) {
            keys.insert(key);
        }
        rest = &after[end + 1..];
    }
    keys
}

fn key_ok(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}
