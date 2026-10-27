use sigil::Host;
use sigil::events::{
    EventRouter, ModifiersState, MouseButton, MouseButtons, PointerEvent, PointerEventInit,
    PointerKind, PointerPhase, ShellEvent,
};
use sigil::geometry::Point;
use sigil::reactive::Runtime;
use sigil::terminal::Screen;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::super::pane::Pane;
use super::super::view::root;
use super::busy_info;
use crate::tui::Msg;

fn right(phase: PointerPhase, x: f32, y: f32) -> ShellEvent {
    ShellEvent::Pointer(PointerEvent::new(PointerEventInit {
        position: Point::new(x, y),
        delta: Point::new(0.0, 0.0),
        kind: PointerKind::Mouse,
        button: Some(MouseButton::Right),
        buttons: if phase == PointerPhase::Down {
            MouseButtons::RIGHT
        } else {
            MouseButtons::EMPTY
        },
        modifiers: ModifiersState::empty(),
        phase,
    }))
}

#[test]
fn a_right_click_on_a_file_row_opens_its_menu_on_screen() {
    Runtime::scope("pane-context-menu", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(120, 30);
        app.update(Msg::Files(vec!["README.md".to_owned()]));
        app.toggle_pane(Pane::Files);
        app.refresh();
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(120, 30);
        let mut router = EventRouter::new();
        let mut text = String::new();
        for _ in 0..3 {
            text = screen
                .frame(&mut host, &mut router, [], (120, 30))
                .expect("frame")
                .text();
        }
        let (row, col) = text
            .lines()
            .enumerate()
            .find_map(|(row, line)| {
                line.find("README.md")
                    .map(|col| (row, line[..col].chars().count()))
            })
            .expect("the file row shows");
        let cell = Host::CELL;
        let (x, y) = ((col as f32 + 1.5) * cell, (row as f32 + 0.5) * cell);
        let clicks = [
            right(PointerPhase::Down, x, y),
            right(PointerPhase::Up, x, y),
        ];
        screen
            .frame(&mut host, &mut router, clicks, (120, 30))
            .expect("frame");
        for _ in 0..2 {
            text = screen
                .frame(&mut host, &mut router, [], (120, 30))
                .expect("frame")
                .text();
        }
        assert!(text.contains("attach @path"), "{text}");
        assert!(text.contains("insert path"), "{text}");
        assert!(text.contains("open in $EDITOR"), "{text}");
    });
}
