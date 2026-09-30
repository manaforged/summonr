use sigil::prelude::{
    IntoNode, MouseButton, NodeBuilder, NodeEventsExt, NodeStyleExt, Tone, badge, list_row,
};

use super::App;
use super::body::{detail, header};
use crate::mcp::{McpState, McpTransport, ServerStatus};

const EMPTY: &str = "No MCP servers configured. Add one with `summonr mcp add`.";

pub(super) fn mcp(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.get();
    let open = app.mcp_open.peek();
    let cursor = app.mcp_cursor.peek();
    column = column.child(header(
        &app,
        "MCP servers".to_string(),
        model.mcp.len().to_string(),
    ));
    if model.mcp.is_empty() {
        return column.child(dim(EMPTY.to_string()));
    }
    for (index, server) in model.mcp.iter().enumerate() {
        let menu = app.clone();
        let toggle = app.clone();
        column = column.child(
            list_row(index == cursor, server.name.clone(), summary(server))
                .child(badge(server.state.label(), tone(server.state)))
                .pointer_down(move |event| {
                    if event.button == Some(MouseButton::Right) {
                        menu.context_row(index);
                    } else {
                        toggle.mcp_cursor.set(index);
                        toggle.mcp_enter();
                    }
                    event.stop_propagation();
                }),
        );
        if let Some(text) = &server.detail {
            column = column.child(dim(text.clone()));
        }
        if open.as_deref() == Some(server.name.as_str()) {
            column = column.child(detail(
                server
                    .launch
                    .iter()
                    .cloned()
                    .chain(server.tools.iter().cloned()),
            ));
        }
    }
    column
}

fn summary(server: &ServerStatus) -> String {
    format!(
        "{} · {} · {} tools · {}",
        server.state.label(),
        server.transport.map_or("-", McpTransport::label),
        server.tools.len(),
        server.schema_size(),
    )
}

fn tone(state: McpState) -> Tone {
    match state {
        McpState::Live => Tone::Success,
        McpState::Cached | McpState::Unlisted => Tone::Info,
        McpState::Waiting | McpState::Stopped => Tone::Warn,
        McpState::Failed => Tone::Error,
    }
}

fn dim(text: String) -> impl IntoNode + 'static {
    sigil::kernel::text(text)
        .nowrap()
        .class(sigil::style::Token::OnSurface.text())
        .class("text-xs text-gray-11")
}
