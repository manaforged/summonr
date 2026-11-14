use super::super::{
    HOOK_FILE, HOOKS, LSP, MCP, NATIVE_HOOKS, NATIVE_MCP, Plugin, hooks, is_json, json_file,
    located, read_parsed, server_entry,
};
use crate::hooks::Hooks;
use summonr_core::error::{Error, Result};
use summonr_types::mcp::Entry;

pub(super) fn require_tree(plugin: &Plugin) -> Result<()> {
    for path in [plugin.dir.join(MCP.file), plugin.dir.join(LSP.file)] {
        json_file(&path)?;
    }
    read_parsed(&plugin.dir.join(NATIVE_MCP), toml::from_str::<toml::Value>)?;
    read_parsed(&plugin.dir.join(NATIVE_HOOKS), toml::from_str::<Hooks>)?;
    let servers = [MCP.key, LSP.key]
        .into_iter()
        .flat_map(|key| plugin.files(key))
        .filter(|path| is_json(path));
    for path in servers {
        json_file(&path)?;
    }
    let hook_files = std::iter::once(plugin.dir.join(HOOK_FILE)).chain(plugin.files(HOOKS));
    for path in hook_files {
        if let Some(problem) = hooks::shape(&json_file(&path)?) {
            return Err(Error::Config(located(&path, problem)));
        }
    }
    for server in plugin.mcp_servers() {
        server_entry::<Entry, _>(&server.file, &server.key, server.config)
            .map_err(Error::Config)?;
    }
    super::super::lsp::check(plugin)
}
