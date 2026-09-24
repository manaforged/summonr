use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

use crate::config;
use crate::durable::store;
use crate::error::{Error, Result};
use crate::prompt::summonr_home;
use crate::skills;

pub fn add(src: &Path) -> Result<String> {
    let src = src
        .canonicalize()
        .map_err(|error| Error::Config(format!("{}: {error}", src.display())))?;
    if !src.is_dir() {
        return Err(Error::Config(format!(
            "{} is not a directory",
            src.display()
        )));
    }
    let home = summonr_home()?;
    let mut lines = Vec::new();
    copy_skills(&src, &home.join("skills"), &mut lines)?;
    copy_bin(&src, &home.join("bin"), &mut lines)?;
    merge_mcp(&src, &config::path()?, &mut lines)?;
    merge_hooks(&src, &config::path()?, &mut lines)?;
    if lines.is_empty() {
        return Err(Error::Config(
            "nothing to add: need skills/, bin/, mcp.toml, or hooks.toml".to_string(),
        ));
    }
    lines.push("restart this process so MCP aliases join the next prefix_hash".to_string());
    Ok(lines.join("\n"))
}

pub fn list() -> Result<String> {
    let home = summonr_home()?;
    let mut lines = Vec::new();
    push_names("skills", &home.join("skills"), &mut lines);
    push_names("bin", &home.join("bin"), &mut lines);
    let file = config::file()?;
    if !file.mcp.is_empty() {
        lines.push(format!(
            "mcp\t{}",
            file.mcp.keys().cloned().collect::<Vec<_>>().join(" ")
        ));
    }
    let hooks = [
        ("pre_tool", file.hooks.pre_tool.is_some()),
        ("post_tool", file.hooks.post_tool.is_some()),
        ("compact", file.hooks.compact.is_some()),
        ("stop", file.hooks.stop.is_some()),
    ];
    let named: Vec<_> = hooks
        .into_iter()
        .filter_map(|(name, on)| on.then_some(name))
        .collect();
    if !named.is_empty() {
        lines.push(format!("hooks\t{}", named.join(" ")));
    }
    if lines.is_empty() {
        return Ok("no extras".to_string());
    }
    Ok(lines.join("\n"))
}

fn copy_skills(src: &Path, dest_root: &Path, lines: &mut Vec<String>) -> Result<()> {
    if src.join("SKILL.md").is_file() {
        return copy_one_skill(src, dest_root, lines);
    }
    let nested = src.join("skills");
    if !nested.is_dir() {
        return Ok(());
    }
    for entry in read_dirs(&nested)? {
        copy_one_skill(&entry, dest_root, lines)?;
    }
    Ok(())
}

fn copy_one_skill(src: &Path, dest_root: &Path, lines: &mut Vec<String>) -> Result<()> {
    if !src.join("SKILL.md").is_file() {
        return Ok(());
    }
    let name = src
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::Config("skill directory name is not utf-8".to_string()))?;
    if !skills::valid(name) {
        return Err(Error::Config(format!(
            "skill `{name}`: letters, digits, `-`, and `_` only"
        )));
    }
    let dest = dest_root.join(name);
    copy_tree(src, &dest)?;
    lines.push(format!("skill\t{name}\t{}", config::tilde(&dest)));
    Ok(())
}

fn copy_bin(src: &Path, dest_root: &Path, lines: &mut Vec<String>) -> Result<()> {
    let bin = src.join("bin");
    if !bin.is_dir() {
        return Ok(());
    }
    let name = src
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::Config("bundle directory name is not utf-8".to_string()))?;
    if !skills::valid(name) {
        return Err(Error::Config(format!(
            "bin bundle `{name}`: letters, digits, `-`, and `_` only"
        )));
    }
    let dest = dest_root.join(name);
    copy_tree(&bin, &dest)?;
    lines.push(format!(
        "bin\t{name}\t{} (copied; not executed)",
        config::tilde(&dest)
    ));
    Ok(())
}

fn merge_mcp(src: &Path, config: &Path, lines: &mut Vec<String>) -> Result<()> {
    let path = src.join("mcp.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let incoming: DocumentMut = text
        .parse()
        .map_err(|error| Error::Config(format!("mcp.toml: {error}")))?;
    let Some(table) = incoming.get("mcp").and_then(|item| item.as_table()) else {
        return Err(Error::Config(
            "mcp.toml: use [mcp.NAME] tables, same as config.toml".to_string(),
        ));
    };
    let mut document = read_config(config)?;
    for (name, item) in table.iter() {
        if document
            .get("mcp")
            .and_then(|mcp| mcp.get(name))
            .is_some_and(|existing| !existing.is_none())
        {
            return Err(Error::Config(format!(
                "mcp `{name}` already in config.toml; not overwritten"
            )));
        }
        document["mcp"][name] = item.clone();
        lines.push(format!("mcp\t{name}"));
    }
    if let Some(mcp) = document["mcp"].as_table_mut() {
        mcp.set_implicit(true);
    }
    store::write(config, document.to_string())?;
    Ok(())
}

fn merge_hooks(src: &Path, config: &Path, lines: &mut Vec<String>) -> Result<()> {
    let path = src.join("hooks.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let incoming: DocumentMut = text
        .parse()
        .map_err(|error| Error::Config(format!("hooks.toml: {error}")))?;
    let table = incoming
        .get("hooks")
        .and_then(|item| item.as_table())
        .unwrap_or(incoming.as_table());
    let mut document = read_config(config)?;
    for key in ["pre_tool", "post_tool", "compact", "stop"] {
        let Some(item) = table.get(key) else {
            continue;
        };
        if document
            .get("hooks")
            .and_then(|hooks| hooks.get(key))
            .is_some_and(|existing| !existing.is_none())
        {
            return Err(Error::Config(format!(
                "hooks.{key} already in config.toml; not overwritten"
            )));
        }
        document["hooks"][key] = item.clone();
        lines.push(format!("hooks\t{key}"));
    }
    if let Some(hooks) = document["hooks"].as_table_mut() {
        hooks.set_implicit(true);
    }
    store::write(config, document.to_string())?;
    Ok(())
}

fn read_config(path: &Path) -> Result<DocumentMut> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    text.parse()
        .map_err(|error| Error::Config(format!("config.toml: {error}")))
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

fn read_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn push_names(kind: &str, root: &Path, lines: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut names: Vec<_> = entries
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            path.file_name()?.to_str().map(str::to_string)
        })
        .filter(|name| skills::valid(name))
        .collect();
    names.sort();
    if !names.is_empty() {
        lines.push(format!("{kind}\t{}", names.join(" ")));
    }
}

pub(crate) fn bin_path_prefix() -> Option<String> {
    let root = summonr_home().ok()?.join("bin");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return None;
    };
    let mut dirs: Vec<_> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    if dirs.is_empty() {
        return None;
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    Some(
        dirs.iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(&sep.to_string()),
    )
}
