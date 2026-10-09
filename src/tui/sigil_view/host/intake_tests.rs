use super::*;
use crate::tui::sigil_view::view::root;
use sigil::Host;
use sigil::events::EventRouter;
use sigil::terminal::Screen;

#[test]
fn bracketed_paste_normalizes_folds_and_keeps_shift_enter_in_the_editor() {
    Runtime::scope("terminal-paste", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(profile::info(), workspace.path().to_owned(), commands);
        assert!(matches!(sent.try_recv(), Ok(Cmd::Offers)));
        app.resize(100, 30);
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(100, 30);
        let mut router = EventRouter::new();
        for _ in 0..2 {
            screen
                .frame(&mut host, &mut router, [], (100, 30))
                .expect("focus composer");
        }
        let mut events = vec![sigil::terminal::paste("first\r\nsecond\rthird")];
        app.intake(&mut events);
        screen
            .frame(&mut host, &mut router, events, (100, 30))
            .expect("paste normalized text");
        assert_eq!(app.draft.peek(), "first\nsecond\nthird");
        let mut events = vec![
            sigil::terminal::map_key(&crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::SHIFT,
            ))
            .expect("map modified enter"),
        ];
        app.intake(&mut events);
        screen
            .frame(&mut host, &mut router, events, (100, 30))
            .expect("insert newline through intake");
        assert_eq!(app.draft.peek(), "first\nsecond\nthird\n");
        assert!(sent.try_recv().is_err(), "modified enter submitted");
        let long = "fold this line\r\n".repeat(crate::limits::limits().paste_fold_lines);
        let mut events = vec![sigil::terminal::paste(&long)];
        app.intake(&mut events);
        assert!(events.is_empty(), "long paste bypassed folding");
        assert!(app.draft.peek().contains("[pasted "));
        assert!(
            app.expand_pastes(&app.draft.peek())
                .contains(&long.replace("\r\n", "\n"))
        );
    });
}

#[test]
fn bracketed_image_paths_attach_without_submitting_and_secret_pastes_stay_private() {
    Runtime::scope("terminal-image-secret", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let path = workspace.path().join("shot.png");
        image::RgbaImage::new(2, 2)
            .save(&path)
            .expect("image fixture");
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(profile::info(), workspace.path().to_owned(), commands);
        assert!(matches!(sent.try_recv(), Ok(Cmd::Offers)));
        let enter = sigil::terminal::map_key(&crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ))
        .expect("map enter");
        let mut events = vec![sigil::terminal::paste(&path.display().to_string()), enter];
        app.intake(&mut events);
        assert!(events.is_empty());
        assert_eq!(app.attached.peek(), 1);
        assert!(app.draft.peek().is_empty());
        assert!(sent.try_recv().is_err());
        let (request, answer) = crate::askpass::request("Fixture password:");
        app.secret_open(request);
        let mut events = vec![sigil::terminal::paste("fixture-password\r\nignored")];
        app.intake(&mut events);
        assert!(events.is_empty());
        assert!(app.draft.peek().is_empty());
        app.secret_finish(true);
        let secret = answer
            .try_recv()
            .expect("secret answer")
            .expect("submitted secret");
        assert_eq!(secret.as_str(), "fixture-password");
    });
}

use crate::tui::sigil_view::profile;
use crate::tui::{Activity, Cmd};
use sigil::reactive::Runtime;
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn image_paste_shortcuts_queue_the_clipboard_image_during_a_turn() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::new(2, 2))
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode the clipboard fixture");
    for modifier in [
        crossterm::event::KeyModifiers::ALT,
        crossterm::event::KeyModifiers::CONTROL,
    ] {
        Runtime::scope("image-paste-shortcuts", || {
            let workspace = tempfile::tempdir().expect("workspace");
            let (commands, mut sent) = unbounded_channel();
            let app = App::new(profile::info(), workspace.path().to_owned(), commands);
            assert!(matches!(sent.try_recv(), Ok(Cmd::Offers)));
            app.update(Msg::Busy(Some(Activity::Turn)));
            let mut events = vec![
                sigil::terminal::map_key(&crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('v'),
                    modifier,
                ))
                .expect("map the paste shortcut"),
            ];
            app.intake(&mut events);
            assert!(events.is_empty(), "the paste shortcut reached the editor");
            assert!(matches!(sent.try_recv(), Ok(Cmd::Clipboard)));
            app.update(Msg::Clipboard(Ok(png.get_ref().clone())));
            assert_eq!(app.attached.peek(), 1);
            app.submit();
            let Cmd::Queue(input) = sent.try_recv().expect("queue the image-only input") else {
                panic!("the image was not queued during the turn");
            };
            assert_eq!(input.images.len(), 1);
            assert_eq!(input.images[0].media_type, "image/png");
            assert_eq!(
                input.images[0].bytes().expect("decode the queued image"),
                *png.get_ref()
            );
            assert!(input.text.is_empty());
        });
    }
}
