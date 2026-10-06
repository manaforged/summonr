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
fn copying_after_an_image_says_the_picture_stays() {
    use crate::tui::sigil_view::model::Entry;

    Runtime::scope("copy-image", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, _sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.model_mut(|model| {
            model.entries.push(Entry::Assistant {
                text: "older reply".to_string(),
                done: true,
            });
            model.entries.push(Entry::Image {
                source: sigil::prelude::ImageSource::from_rgba8(1, 1, vec![0, 0, 0, 255])
                    .expect("1x1 rgba"),
                bytes: std::rc::Rc::from([0_u8, 0, 0, 255].as_slice()),
                label: "shot".to_string(),
            });
        });
        app.copy_chat(None);
        let note = app
            .model
            .get()
            .entries
            .iter()
            .rev()
            .find_map(|entry| match entry {
                Entry::System(text) => Some(text.clone()),
                _ => None,
            });
        assert_eq!(note.as_deref(), Some("the picture stays in the session"));
        assert!(app.copy.get().is_none());
    });
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
        app.submit();
        let mut queued = None;
        while let Ok(command) = sent.try_recv() {
            if let Cmd::Queue(input) = command {
                queued = Some(input);
            }
        }
        assert_eq!(queued.map(|input| input.images.len()), Some(1));
    });
}
