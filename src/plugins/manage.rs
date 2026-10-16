use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{MANIFEST, Plugin, data_dir, installed, json_file, require_tree, root, unknown_off};
use crate::error::{Error, Result};
use crate::skills;

mod report;

use report::{describe, report, servers, settings_servers, skipped};

pub struct Installed {
    pub name: String,
    pub dir: PathBuf,
    pub report: String,
    servers: Vec<Server>,
}

struct Server {
    name: String,
    needs_config: bool,
}

pub fn install(src: &Path) -> Result<Installed> {
    let src = src
        .canonicalize()
        .map_err(|error| Error::Config(format!("{}: {error}", src.display())))?;
    if !src.is_dir() {
        return Err(Error::Config(format!(
            "{} is not a directory",
            src.display()
        )));
    }
    let manifest = json_file(&src.join(MANIFEST))?;
    let plugin_name = manifest["name"]
        .as_str()
        .map(str::to_string)
        .or_else(|| src.file_name()?.to_str().map(str::to_string))
        .unwrap_or_default();
    if !skills::valid(&plugin_name) {
        return Err(Error::Config(format!(
            "plugin name `{plugin_name}`: letters, digits, `-`, and `_` only"
        )));
    }
    let source = Plugin::at(plugin_name.clone(), src);
    require_tree(&source)?;
    if describe(&source).is_empty() {
        let mut lines = vec![format!("plugin `{plugin_name}` adds nothing summonr loads")];
        lines.extend(skipped(&source));
        return Err(Error::Config(lines.join("\n")));
    }
    let plugins = root()?;
    let stage = plugins.join(format!(".{plugin_name}.installing"));
    stage_copy(&source.dir, &stage)?;
    let dest = place(&plugins, &stage, &plugin_name)?;
    let plugin = Plugin::at(plugin_name, dest);
    let off = crate::config::file()?
        .plugins
        .off
        .iter()
        .any(|item| item == &plugin.name);
    let configured = settings_servers(&plugin);
    let servers = if off {
        Vec::new()
    } else {
        servers(&plugin)
            .into_iter()
            .map(|name| Server {
                // complexity: bounded by a BTreeSet lookup per server, O(log n)
                needs_config: configured.contains(&name),
                name,
            })
            .collect()
    };
    Ok(Installed {
        report: report(&plugin, &manifest, off),
        name: plugin.name,
        dir: plugin.dir,
        servers,
    })
}

pub async fn check_servers(installed: &Installed, workspace: &Path) -> Result<Vec<String>> {
    let probes = futures::future::join_all(
        installed
            .servers
            .iter()
            .map(|server| crate::mcp::probe(workspace, &server.name)),
    )
    .await;
    let mut lines = Vec::new();
    for (server, probed) in installed.servers.iter().zip(probes) {
        let name = &server.name;
        lines.push(match probed {
            Ok(Some(count)) => format!("{name}: {count} tools"),
            Ok(None) => format!("{name}: off in config"),
            Err(error) if server.needs_config => {
                format!("{name}: waiting for its user_config values: {error}")
            }
            Err(error) => {
                return Err(Error::Setup(format!(
                    "{name}: not reachable: {error}\nkept in {}; fix the command or run `summonr uninstall {}`",
                    crate::config::tilde(&installed.dir),
                    installed.name
                )));
            }
        });
    }
    Ok(lines)
}

pub fn uninstall(plugin_name: &str) -> Result<bool> {
    if !skills::valid(plugin_name) {
        return Ok(false);
    }
    let dir = root()?.join(plugin_name);
    let removed = if dir.is_dir() {
        std::fs::remove_dir_all(&dir)?;
        true
    } else {
        false
    };
    let data_error = match data_dir(plugin_name) {
        Ok(data) if data.is_dir() => std::fs::remove_dir_all(data).err(),
        Ok(_) | Err(_) => None,
    };
    let cleared = crate::config::drop_plugin_off(&crate::config::path()?, plugin_name)?;
    if let Some(error) = data_error {
        return Err(error.into());
    }
    Ok(removed || cleared)
}

pub fn list() -> Result<String> {
    let config = crate::config::file()?;
    let off_set: BTreeSet<&str> = config.plugins.off.iter().map(String::as_str).collect();
    let lines: Vec<String> = installed()
        .iter()
        .map(|plugin| {
            let state = if off_set.contains(plugin.name.as_str()) {
                "off"
            } else {
                "on"
            };
            let mut adds = describe(plugin);
            adds.extend(plugin.skipped_mods());
            let adds = if adds.is_empty() {
                "adds nothing summonr loads".to_string()
            } else {
                adds.join("; ")
            };
            format!("{}\t{state}\t{adds}", plugin.name)
        })
        .collect();
    let mut lines = if lines.is_empty() {
        vec!["no plugins installed".to_string()]
    } else {
        lines
    };
    lines.extend(unknown_off(&config.plugins.off));
    Ok(lines.join("\n"))
}

fn stage_copy(src: &Path, stage: &Path) -> Result<()> {
    if stage.exists() {
        std::fs::remove_dir_all(stage)?;
    }
    if let Err(error) = copy_tree(src, stage) {
        return match std::fs::remove_dir_all(stage) {
            Ok(()) => Err(error),
            Err(_) if !stage.exists() => Err(error),
            Err(cleanup) => Err(Error::Config(format!(
                "{error}; failed to remove {}: {cleanup}",
                stage.display()
            ))),
        };
    }
    Ok(())
}

fn copy_tree(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_name().to_str() == Some(".git") {
            continue;
        }
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn place(root: &Path, stage: &Path, plugin_name: &str) -> Result<PathBuf> {
    let dest = root.join(plugin_name);
    let backup = root.join(format!(".{plugin_name}.previous"));
    if backup.exists() {
        std::fs::remove_dir_all(&backup)?;
    }
    if dest.exists() {
        std::fs::rename(&dest, &backup)?;
    }
    if let Err(error) = std::fs::rename(stage, &dest) {
        let note = if !backup.exists() {
            String::new()
        } else {
            match std::fs::rename(&backup, &dest) {
                Ok(()) => String::new(),
                Err(restore) => format!("; previous copy is at {}: {restore}", backup.display()),
            }
        };
        return Err(Error::Config(format!(
            "could not install `{plugin_name}`: {error}{note}"
        )));
    }
    if backup.exists() {
        std::fs::remove_dir_all(&backup)?;
    }
    Ok(dest)
}
