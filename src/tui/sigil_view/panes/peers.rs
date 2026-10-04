use sigil::prelude::Paint;
use sigil::prelude::{MouseButton, NodeBuilder, NodeEventsExt, Tone, chip, list_row};

use super::App;
use super::body::header;

pub(super) fn peers(app: App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.get();
    let cursor = app.peer_cursor.peek();
    column = column.child(header(
        &app,
        "Peers".to_string(),
        model.peers.len().to_string(),
    ));
    if model.peers.is_empty() {
        return column.child(
            sigil::kernel::text("no other live sessions")
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        );
    }
    for (index, agent) in model.peers.iter().enumerate() {
        let menu = app.clone();
        let pick = app.clone();
        let tone = if agent.status == "working" {
            Tone::Info
        } else {
            Tone::Neutral
        };
        let kind = if agent.kind.is_empty() || agent.kind == "summonr" {
            String::new()
        } else {
            format!("{} · ", agent.kind)
        };
        let dir = agent
            .cwd
            .rsplit(['/', '\\'])
            .find(|part| !part.is_empty())
            .unwrap_or(&agent.cwd);
        column = column.child(
            list_row(
                index == cursor,
                format!("@{}", agent.label()),
                format!("{kind}{} · {dir}", agent.model),
            )
            .child(chip(agent.status.clone(), tone))
            .pointer_down(move |event| {
                if event.button == Some(MouseButton::Right) {
                    menu.context_row(index);
                } else {
                    pick.peer_cursor.set(index);
                    pick.peer_enter();
                }
                event.stop_propagation();
            }),
        );
    }
    column
}
