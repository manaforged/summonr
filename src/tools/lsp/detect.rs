use super::*;

pub(in crate::tools) fn detect(workspace: &Path, path: &Path) -> Result<Launch> {
    if let Some(module) = workspace_module(workspace)? {
        return built(language::named(&module)?);
    }
    let config = crate::config::file()?;
    if let Some(command) = config.lsp.command {
        return custom(&command);
    }
    if let Some(module) = config.lsp.module {
        return built(language::named(&module)?);
    }
    from_tree(workspace, path)
}

pub(in crate::tools) fn available(workspace: &Path) -> bool {
    if let Ok(Some(module)) = workspace_module(workspace) {
        return language::named(&module).is_ok_and(|module| built(module).is_ok());
    }
    if let Ok(config) = crate::config::file() {
        if config.lsp.command.is_some() {
            return true;
        }
        if let Some(module) = config.lsp.module {
            return language::named(&module).is_ok_and(|module| built(module).is_ok());
        }
    }
    language::present(workspace)
        .into_iter()
        .any(|module| built(module).is_ok())
}

pub(super) fn from_tree(workspace: &Path, path: &Path) -> Result<Launch> {
    built(language::detect(workspace, path)?)
}

pub(super) fn workspace_module(workspace: &Path) -> Result<Option<String>> {
    let path = workspace.join(".summonr/config.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let config = crate::config::parse(&path)?;
    if config.lsp.command.is_some() {
        return Err(Error::Config(
            "workspace .summonr/config.toml cannot set [lsp] command; use a curated module or put the command in ~/.summonr/config.toml"
                .to_string(),
        ));
    }
    Ok(config.lsp.module)
}

pub(super) fn built(module: &language::Module) -> Result<Launch> {
    let mut names = Vec::new();
    for server in module.servers {
        names.push(server.name);
        if on_path(server.name) {
            let mut command = vec![server.name.to_string()];
            command.extend(server.args.iter().map(|arg| (*arg).to_string()));
            return Ok(Launch {
                key: module.id.to_string(),
                argv: command,
                options: language::options(module),
            });
        }
    }
    Err(Error::Tool(format!(
        "{} not on PATH for the {} module; install one or set [lsp] command in ~/.summonr/config.toml",
        names.join(" or "),
        module.id
    )))
}

pub(super) fn custom(command: &[String]) -> Result<Launch> {
    Ok(Launch {
        key: "custom".to_string(),
        argv: argv(command)?,
        options: json!({}),
    })
}

pub(super) fn argv(command: &[String]) -> Result<Vec<String>> {
    let name = command
        .first()
        .map(String::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Error::Tool("[lsp] command is empty".to_string()))?;
    if !on_path(name) {
        return Err(Error::Tool(format!(
            "{name} is not on PATH; fix [lsp] command in ~/.summonr/config.toml"
        )));
    }
    Ok(command.to_vec())
}

pub(super) fn on_path(name: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        dir.join(name).is_file()
            || (cfg!(windows)
                && Path::new(name).extension().is_none()
                && dir.join(format!("{name}.exe")).is_file())
    })
}
