use std::collections::BTreeMap;
use std::sync::Mutex;

use super::*;
use crate::askpass::on_path;
use crate::plugins::lsp::{Server, servers};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::tools) enum Scope {
    Request,
    Edit,
}

#[derive(Clone)]
enum Source {
    Pinned(&'static language::Module),
    Custom(Arc<Launch>),
    Tree(Arc<[Server]>),
}

#[derive(Default)]
pub(in crate::tools) struct Launches {
    source: Mutex<Option<Source>>,
    built: Mutex<BTreeMap<&'static str, Arc<Launch>>>,
}

impl Launches {
    pub(in crate::tools) fn launch(
        &self,
        workspace: &Path,
        path: &Path,
        scope: Scope,
    ) -> Result<Arc<Launch>> {
        match self.source(workspace)? {
            Source::Custom(launch) => Ok(launch),
            Source::Pinned(module) => {
                if scope == Scope::Edit && !handles(module, path) {
                    return Err(Error::Tool(format!(
                        "the {} module does not check `{}`",
                        module.id,
                        path.display()
                    )));
                }
                self.built(module)
            }
            Source::Tree(plugins) => match plugins.iter().find(|server| server.handles(path)) {
                Some(server) => plugin(server).map(Arc::new),
                None => self.built(language::detect(workspace, path)?),
            },
        }
    }

    fn source(&self, workspace: &Path) -> Result<Source> {
        let mut source = lock(&self.source)?;
        if let Some(known) = source.as_ref() {
            return Ok(known.clone());
        }
        let found = resolve(workspace)?;
        *source = Some(found.clone());
        Ok(found)
    }

    fn built(&self, module: &'static language::Module) -> Result<Arc<Launch>> {
        if let Some(launch) = lock(&self.built)?.get(module.id) {
            return Ok(launch.clone());
        }
        let launch = Arc::new(built(module)?);
        lock(&self.built)?.insert(module.id, launch.clone());
        Ok(launch)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>> {
    mutex
        .lock()
        .map_err(|_poison| Error::Tool("lsp lock poisoned".to_string()))
}

fn resolve(workspace: &Path) -> Result<Source> {
    if let Some(module) = workspace_module(workspace)? {
        return Ok(Source::Pinned(language::named(&module)?));
    }
    let config = crate::config::file()?;
    if let Some(command) = &config.lsp.command {
        return Ok(Source::Custom(Arc::new(custom(command)?)));
    }
    if let Some(module) = &config.lsp.module {
        return Ok(Source::Pinned(language::named(module)?));
    }
    Ok(Source::Tree(servers(workspace).into()))
}

fn handles(module: &language::Module, path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| module.extensions.contains(&extension))
}

pub(in crate::tools) fn available(workspace: &Path) -> bool {
    if let Ok(Some(module)) = workspace_module(workspace) {
        return language::named(&module).is_ok_and(|module| built(module).is_ok());
    }
    if let Ok(config) = crate::config::file() {
        if config.lsp.command.is_some() {
            return true;
        }
        if let Some(module) = &config.lsp.module {
            return language::named(module).is_ok_and(|module| built(module).is_ok());
        }
    }
    if servers(workspace)
        .iter()
        .any(|server| plugin(server).is_ok())
    {
        return true;
    }
    language::present(workspace)
        .into_iter()
        .any(|module| built(module).is_ok())
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
        if on_path(server.name).is_some() {
            let mut command = vec![server.name.to_string()];
            command.extend(server.args.iter().map(|arg| (*arg).to_string()));
            return Ok(Launch {
                key: module.id.to_string(),
                argv: command,
                env: Vec::new(),
                languages: BTreeMap::new(),
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
        env: Vec::new(),
        languages: BTreeMap::new(),
        options: json!({}),
    })
}

fn plugin(server: &Server) -> Result<Launch> {
    let name = server.argv.first().map(String::as_str).unwrap_or_default();
    if !Path::new(name).is_file() && on_path(name).is_none() {
        return Err(Error::Tool(format!(
            "{name} is not on PATH for the {} plugin server",
            server.key
        )));
    }
    Ok(Launch {
        key: server.key.clone(),
        argv: server.argv.clone(),
        env: server.env.clone().into_iter().collect(),
        languages: server.languages.clone(),
        options: json!({}),
    })
}

pub(super) fn argv(command: &[String]) -> Result<Vec<String>> {
    let name = command
        .first()
        .map(String::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Error::Tool("[lsp] command is empty".to_string()))?;
    if on_path(name).is_none() {
        return Err(Error::Tool(format!(
            "{name} is not on PATH; fix [lsp] command in ~/.summonr/config.toml"
        )));
    }
    Ok(command.to_vec())
}
