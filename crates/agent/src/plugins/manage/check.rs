use super::super::{HOOKS, LSP, MCP, Plugin, is_json, json_file, read_parsed, server_entry};
use crate::hooks::Hooks;
use summonr_core::error::{Error, Result};
use summonr_types::mcp::Entry;

pub(super) fn require_tree(plugin: &Plugin) -> Result<()> {
    for path in [
        plugin.dir.join(MCP.file),
        plugin.dir.join("hooks/hooks.json"),
        plugin.dir.join(LSP.file),
    ] {
        json_file(&path)?;
    }
    read_parsed(&plugin.dir.join("mcp.toml"), toml::from_str::<toml::Value>)?;
    read_parsed(&plugin.dir.join("hooks.toml"), toml::from_str::<Hooks>)?;
    let servers = [MCP.key, LSP.key]
        .into_iter()
        .flat_map(|key| plugin.files(key))
        .filter(|path| is_json(path));
    for path in plugin.files(HOOKS).into_iter().chain(servers) {
        json_file(&path)?;
    }
    for server in plugin.mcp_servers() {
        server_entry::<Entry, _>(&server.file, &server.key, server.config)
            .map_err(Error::Config)?;
    }
    super::super::lsp::check(plugin)
}
