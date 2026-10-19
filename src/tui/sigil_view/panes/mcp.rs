use sigil::prelude::Paint;
use sigil::prelude::{IntoNode, MouseButton, NodeBuilder, NodeEventsExt, Tone, chip, list_row};

use super::App;
use super::body::{detail, header};
use crate::mcp::{McpState, McpTransport, ServerStatus};
use crate::supervise::ServerKind;

const EMPTY: &str = "no mcp servers; summonr mcp add configures one";

pub(super) fn mcp(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.get();
    let open = app.mcp_open.peek();
    let cursor = app.mcp_cursor.peek();
    column = column.child(header(
        &app,
        "Servers".to_string(),
        super::body::count(model.mcp.len()),
    ));
    if model.mcp.is_empty() {
        return column.child(dim(EMPTY.to_string()));
    }
    for (index, server) in model.mcp.iter().enumerate() {
        let menu = app.clone();
        let toggle = app.clone();
        column = column.child(
            list_row(index == cursor, server.name.clone(), summary(server))
                .child(chip(server.state.label(), tone(server.state)))
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
    let head = match server.kind {
        ServerKind::Lsp => "language server".to_string(),
        ServerKind::Mcp => format!(
            "{} · {} · {}",
            server.transport.map_or("-", McpTransport::label),
            crate::text::plural(server.tools.len(), "tool"),
            server.schema_size(),
        ),
    };
    match server.rss_kb {
        Some(kb) => format!("{head} · mem {}", crate::text::megabytes(kb)),
        None => head,
    }
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
        .text_color(sigil::prelude::Token::OnSurface)
        .font_size(12.0)
        .text_color(sigil::prelude::Token::Gray11)
}
