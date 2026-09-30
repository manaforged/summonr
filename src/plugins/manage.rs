use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use super::{MANIFEST, Plugin, UNSUPPORTED, USER_CONFIG, data_dir, installed, read_json, root};
use crate::error::{Error, Result};
use crate::skills;

pub fn install(src: &Path) -> Result<String> {
    let src = src
        .canonicalize()
        .map_err(|error| Error::Config(format!("{}: {error}", src.display())))?;
    if !src.is_dir() {
        return Err(Error::Config(format!(
            "{} is not a directory",
            src.display()
        )));
    }
    let manifest = read_json(&src.join(MANIFEST));
    let name = manifest["name"]
        .as_str()
        .map(str::to_string)
        .or_else(|| src.file_name()?.to_str().map(str::to_string))
        .unwrap_or_default();
    if !skills::valid(&name) {
        return Err(Error::Config(format!(
            "plugin name `{name}`: letters, digits, `-`, and `_` only"
        )));
    }
    let dest = root()?.join(&name);
    if dest.exists() {
        return Err(Error::Config(format!(
            "plugin `{name}` is installed; run `summonr uninstall {name}` first"
        )));
    }
    copy_tree(&src, &dest)?;
    let plugin = Plugin { name, dir: dest };
    let mut lines = vec![format!(
        "plugin\t{}\t{}",
        plugin.name,
        crate::config::tilde(&plugin.dir)
    )];
    lines.extend(describe(&plugin));
    lines.extend(
        UNSUPPORTED
            .iter()
            .filter(|(path, _)| plugin.dir.join(path).exists())
            .map(|(path, what)| format!("skipped\t{path} (summonr does not load plugin {what})")),
    );
    lines.extend(settings(&plugin, &manifest));
    lines.push("restart summonr to load it".to_string());
    Ok(lines.join("\n"))
}

pub fn uninstall(name: &str) -> Result<bool> {
    if !skills::valid(name) {
        return Ok(false);
    }
    let dir = root()?.join(name);
    if !dir.is_dir() {
        return Ok(false);
    }
    std::fs::remove_dir_all(dir)?;
    if let Ok(data) = data_dir(name)
        && data.is_dir()
    {
        std::fs::remove_dir_all(data)?;
    }
    Ok(true)
}

pub fn list() -> Result<String> {
    let off = crate::config::file()?.plugins.off;
    let lines: Vec<String> = installed()
        .iter()
        .map(|plugin| {
            let state = if off.contains(&plugin.name) {
                "off"
            } else {
                "on"
            };
            format!("{}\t{state}\t{}", plugin.name, describe(plugin).join("; "))
        })
        .collect();
    if lines.is_empty() {
        return Ok("no plugins installed".to_string());
    }
    Ok(lines.join("\n"))
}

fn settings(plugin: &Plugin, manifest: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    if manifest.get("userConfig").is_some() {
        lines.push("skipped\tuserConfig (summonr does not prompt for plugin settings)".to_string());
    }
    for map in plugin.mcp_json() {
        for (name, entry) in map.as_object().into_iter().flatten() {
            if entry.to_string().contains(USER_CONFIG) {
                lines.push(format!(
                    "warning\tmcp {name} uses ${{user_config.*}}; summonr leaves it empty, so set the value in [mcp.{}-{name}] yourself",
                    plugin.name
                ));
            }
        }
    }
    lines
}

fn describe(plugin: &Plugin) -> Vec<String> {
    let mut parts = Vec::new();
    let skills: usize = plugin
        .skill_dirs()
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .map(Iterator::count)
        .sum();
    if skills > 0 {
        parts.push(format!("skills {skills}"));
    }
    let servers: BTreeMap<String, ()> = plugin
        .mcp_json()
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|map| map.keys().cloned())
        .chain(
            plugin
                .mcp_toml()
                .and_then(|table| {
                    table
                        .as_table()
                        .map(|table| table.keys().cloned().collect::<Vec<_>>())
                })
                .unwrap_or_default(),
        )
        .map(|name| (name, ()))
        .collect();
    if !servers.is_empty() {
        parts.push(format!(
            "mcp {}",
            servers.into_keys().collect::<Vec<_>>().join(" ")
        ));
    }
    let hooks = plugin.hooks().names();
    if !hooks.is_empty() {
        parts.push(format!("hooks {}", hooks.join(" ")));
    }
    if plugin.bin().is_some() {
        parts.push("bin".to_string());
    }
    parts
}

fn copy_tree(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dest.join(name);
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
