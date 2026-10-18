use super::*;
use crate::mcp::{McpState, McpTransport, ServerStatus};
use crate::supervise::ServerKind;

fn server(name: &str, state: McpState, schema_bytes: usize, detail: Option<&str>) -> ServerStatus {
    ServerStatus {
        name: name.to_owned(),
        kind: ServerKind::Mcp,
        rss_kb: None,
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

#[test]
fn a_page_command_on_the_start_screen_opens_its_pane() {
    sigil::reactive::Runtime::scope("summonr-splash", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let base = Info {
            splash: true,
            ..busy_info()
        };
        let app = super::App::new(base, workspace.path().to_owned(), commands);
        app.resize(160, 40);
        assert!(app.splash.peek());
        app.replace("/mcp".to_owned());
        app.submit();
        assert!(!app.splash.peek());
        assert_eq!(
            app.layout.peek().secondary,
            Some(super::super::pane::Pane::Mcp)
        );
    });
}

#[test]
fn a_refresh_leaves_the_start_screen_once_the_transcript_has_an_entry() {
    sigil::reactive::Runtime::scope("summonr-refresh", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let base = Info {
            splash: true,
            ..busy_info()
        };
        let app = super::App::new(base, workspace.path().to_owned(), commands);
        app.resize(160, 40);
        app.refresh();
        assert!(app.splash.peek());
        app.update(Msg::System("checked the workspace".to_owned()));
        assert!(app.splash.peek(), "the update itself left the start screen");
        app.refresh();
        assert!(!app.splash.peek());
    });
}
