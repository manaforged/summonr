use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub(super) fn profile(
    workspace: &Path,
    temp: &Path,
    write: &[PathBuf],
    deny_network: bool,
    read_only: bool,
) -> Result<String> {
    let workspace = path_string(workspace)?;
    let temp = path_string(temp)?;
    let extra = write
        .iter()
        .map(|path| Ok(format!("    (subpath \"{}\")\n", path_string(path)?)))
        .collect::<Result<String>>()?;
    let network_rule = if deny_network {
        String::from("(deny network*)\n")
    } else {
        String::new()
    };
    let (workspace_rule, workspace_deny) = if read_only {
        (
            String::new(),
            format!("(deny file-write* (subpath \"{workspace}\"))\n"),
        )
    } else {
        (
            format!("    (subpath \"{workspace}\")\n"),
            format!(
                "(deny file-write* (literal \"{workspace}/.git\") (subpath \"{workspace}/.git/hooks\") (literal \"{workspace}/.git/config\"))\n"
            ),
        )
    };
    Ok(format!(
        "(version 1)\n\
(allow default)\n\
{network_rule}\
(deny file-write*)\n\
(allow file-write*\n\
    (literal \"/dev/null\")\n\
{workspace_rule}\
{extra}\
    (subpath \"{temp}\")\n\
 )\n\
{workspace_deny}"
    ))
}

fn path_string(path: &Path) -> Result<String> {
    let path = path.to_str().ok_or_else(|| {
        Error::Config(format!(
            "sandbox path is not valid UTF-8: {}",
            path.display()
        ))
    })?;
    Ok(path.replace('\\', "\\\\").replace('"', "\\\""))
}
