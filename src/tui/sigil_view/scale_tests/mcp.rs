use super::*;
use crate::mcp::{McpState, McpTransport, ServerStatus};

fn server(name: &str, state: McpState, schema_bytes: usize, detail: Option<&str>) -> ServerStatus {
    ServerStatus {
        name: name.to_owned(),
        transport: Some(McpTransport::Stdio),
        state,
        tools: vec!["search".to_owned()],
        schema_bytes,
        launch: Some(format!("{name}-mcp")),
        detail: detail.map(str::to_owned),
    }
}

#[test]
fn the_mcp_pane_shows_each_server_with_its_state_size_and_last_error() {
    let shown = frame_with(busy_info(), 160, 40, State::Idle, |app| {
        app.replace("/mcp".to_owned());
        app.submit();
        app.update(Msg::Mcp(vec![
            server("github", McpState::Live, 2048, None),
            server(
                "broken",
                McpState::Failed,
                0,
                Some("cannot start `broken-mcp`"),
            ),
        ]));
    })
    .join("\n");
    for needle in [
        "github",
        "running",
        "2.0 KB",
        "broken",
        "failed",
        "cannot start",
    ] {
        assert!(shown.contains(needle), "{needle} missing:\n{shown}");
    }
}
