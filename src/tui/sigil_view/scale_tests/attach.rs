use sigil::reactive::Runtime;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::{State, busy_info, frame_with};
use crate::tui::{Activity, Cmd, Msg};

fn paste_png(app: &App) {
    let file = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
    image::RgbaImage::new(2, 2).save(file.path()).unwrap();
    assert!(app.paste(&file.path().display().to_string()));
}

#[test]
fn an_image_pasted_during_a_turn_shows_as_attached() {
    let lines = frame_with(busy_info(), 110, 34, State::Busy, |app| {
        paste_png(app);
    });
    assert!(
        lines.iter().any(|line| line.contains("1 image attached")),
        "{}",
        lines.join("\n")
    );
}

#[test]
fn an_image_only_draft_queues_during_a_turn() {
    Runtime::scope("attach-queue", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.update(Msg::Busy(Some(Activity::Turn)));
        paste_png(&app);
        assert!(app.queue_if_busy());
        let mut queued = None;
        while let Ok(command) = sent.try_recv() {
            if let Cmd::Queue(input) = command {
                queued = Some(input);
            }
        }
        assert_eq!(queued.map(|input| input.images.len()), Some(1));
    });
}
