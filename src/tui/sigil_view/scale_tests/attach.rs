use sigil::reactive::Runtime;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::{State, busy_info, frame_with, paint};
use crate::tui::{Activity, Cmd, Msg};

fn paste_png(app: &App) {
    let file = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .expect("image file");
    image::RgbaImage::new(480, 320)
        .save(file.path())
        .expect("PNG fixture");
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
fn steering_an_image_only_draft_goes_into_the_running_turn() {
    Runtime::scope("attach-queue", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.update(Msg::Busy(Some(Activity::Turn)));
        paste_png(&app);
        app.submit();
        let mut steered = None;
        while let Ok(command) = sent.try_recv() {
            if let Cmd::Steer(input) = command {
                steered = Some(input);
            }
        }
        assert_eq!(steered.map(|input| input.images.len()), Some(1));
    });
}

#[test]
fn an_attached_image_keeps_its_info_band_during_active_and_queued_turns() {
    let note = |state, queued| {
        let grid = paint(busy_info(), 110, 34, state, |app| {
            app.update(Msg::Queued {
                steering: 0,
                queued,
            });
            paste_png(app);
        });
        let shown = grid.text();
        let row = shown
            .lines()
            .position(|line| line.contains("1 image attached"))
            .expect("attachment notice");
        let cell = (0..grid.cols())
            .filter_map(|col| grid.get(col, row))
            .find(|cell| cell.text == "1")
            .expect("image count cell");
        let caption = shown.lines().nth(row).expect("notice row").trim();
        if state != State::Idle {
            assert!(shown.contains("thinking"), "{shown}");
            if queued > 0 {
                assert!(shown.contains("1 queued"), "{shown}");
            }
        }
        (cell.fg, cell.bg, caption.to_owned())
    };
    let idle = note(State::Idle, 0);
    for (state, queued) in [(State::Busy, 0), (State::Tool, 0), (State::Busy, 1)] {
        assert_eq!(note(state, queued), idle, "{state:?}, queued={queued}");
    }
}

#[test]
fn an_image_without_terminal_graphics_keeps_its_label_without_a_blank_preview() {
    let lines = frame_with(busy_info(), 110, 34, State::Idle, |app| {
        paste_png(app);
        app.submit();
    });
    let shown = lines.join("\n");
    assert!(shown.contains("[attached image]"), "{shown}");
    assert!(!shown.contains("image ·"), "{shown}");
    let caption = lines
        .iter()
        .position(|line| line.contains("[attached image]"))
        .expect("attachment caption");
    let previous = lines[..caption]
        .iter()
        .rposition(|line| line.contains("Measured layout."))
        .expect("preceding reply");
    assert!(caption - previous <= 5, "{shown}");
}
