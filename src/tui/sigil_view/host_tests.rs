use std::fs::{File, read_to_string, write};
use std::io::{BufWriter, stdout};
use std::mem::ManuallyDrop;
use std::path::PathBuf;

#[cfg(unix)]
use std::fs::{Permissions, set_permissions};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use sigil::prelude::Cx;
use sigil_hal::Point;
use sigil_tui::{EventRouter, Host, Screen, present};
use tokio::sync::mpsc::unbounded_channel;

use super::super::{app::App, model::Entry, tests::info, view::root};
use super::{CopyStatus, Terminal, edit, mouse, osc52, read_event, restore, status};

#[test]
fn terminal_clipboard_requests_are_unconfirmed_and_propagate_output_failures() {
    let mut bytes = Vec::new();
    assert!(matches!(
        osc52(&mut bytes, "café\n").expect("OSC 52"),
        CopyStatus::Requested
    ));
    assert_eq!(bytes, b"\x1b]52;c;Y2Fmw6kK\x07");
    let file = tempfile::NamedTempFile::new().expect("read-only output");
    let mut output = BufWriter::new(File::open(file.path()).expect("read-only handle"));
    assert!(
        osc52(&mut output, "text").is_err(),
        "failed flush must not look successful"
    );

    sigil::reactive::Runtime::scope("summoner-copy-feedback", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.replace("retain this draft".to_string());
        app.copied("first\nsecond", CopyStatus::Requested);
        assert!(app.transcript.peek().contains("copy requested for 2 lines"));
        assert!(app.transcript.peek().contains("unconfirmed"));
        assert!(!app.transcript.peek().contains("copied"));
        app.copied("first\nsecond", CopyStatus::Confirmed);
        assert!(
            app.transcript
                .peek()
                .contains("copied 2 lines to the clipboard")
        );
        assert_eq!(app.draft.peek(), "retain this draft");
        let frame =
            present(move |cx: &mut Cx| cx.add(root(app)), 100, 24, &[]).expect("copy feedback");
        assert!(frame.contains("unconfirmed"), "{frame}");
        assert!(frame.contains("copied 2 lines to the clipboard"), "{frame}");
        if var_os("MAGE_DUMP_FRAME").is_some() {
            println!("clipboard feedback 100x24:\n{frame}");
        }
    });
}

#[test]
fn external_editor_accepts_arguments_and_keeps_file_names_literal() {
    let directory = tempfile::tempdir().expect("editor fixture directory");
    let path = directory.path().join("draft space & %PATH% ! (literal).md");
    write(&path, "original draft").expect("draft");
    #[cfg(windows)]
    let script = directory.path().join("editor fixture.cmd");
    #[cfg(not(windows))]
    let script = directory.path().join("editor fixture.sh");
    #[cfg(windows)]
    write(
        &script,
        "@echo off\r\nsetlocal DisableDelayedExpansion\r\n>\"%~2\" echo %~1\r\n",
    )
    .expect("editor");
    #[cfg(not(windows))]
    {
        write(&script, "#!/bin/sh\nprintf '%s' \"$1\" > \"$2\"\n").expect("editor");
        set_permissions(&script, Permissions::from_mode(0o700)).expect("executable editor");
    }
    edit(&path, &format!("\"{}\" \"edited draft\"", script.display())).expect("configured editor");
    assert_eq!(
        read_to_string(&path).expect("edited file").trim_end(),
        "edited draft"
    );
    assert!(!directory.path().join("draft space").exists());

    let failure = if cfg!(windows) {
        "cmd /D /C exit 7"
    } else {
        "sh -c 'exit 7'"
    };
    assert!(
        edit(&path, failure)
            .expect_err("failed editor")
            .to_string()
            .contains('7')
    );
    assert_eq!(
        read_to_string(&path).expect("retained file").trim_end(),
        "edited draft"
    );
}

#[test]
fn clicking_transcript_text_preserves_typing_and_paste_focus() {
    sigil::reactive::Runtime::scope("summoner-input-focus", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(80, 24);
        app.model
            .borrow_mut()
            .entries
            .push(Entry::System("select this reply".to_string()));
        app.refresh();
        let mut terminal = ManuallyDrop::new(Terminal {
            paste: Default::default(),
            out: BufWriter::new(stdout()),
            focused: true,
            redraw: false,
            anchor: None,
            selection: None,
        });
        sigil_tui::widgets::install(super::Paste {
            text: terminal.paste.clone(),
            copy: app.copy,
        });
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        screen
            .frame(&mut host, &mut router, [], (80, 24))
            .expect("initial frame");
        router.focus_next(host.cx());
        let mut pointer = Point::new(0.0, 0.0);
        for event in [
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 4,
                row: 1,
                modifiers: KeyModifiers::NONE,
            }),
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            Event::Paste(" café\n日本語".to_string()),
        ] {
            let mut incoming = Vec::new();
            read_event(
                event,
                &mut incoming,
                &mut pointer,
                &mut screen,
                &mut terminal,
                &app,
            )
            .expect("input");
            screen
                .frame(&mut host, &mut router, incoming, (80, 24))
                .expect("input frame");
        }
        assert_eq!(app.draft.peek(), "x café\n日本語");
        assert_eq!(app.caret.peek(), "x café\n日本語".len());
        app.toggle_pane(super::super::pane::Pane::Files);
        app.focus_pane(true);
        screen
            .frame(&mut host, &mut router, [], (80, 24))
            .expect("focused pane");

        for (event, expected) in [
            (
                Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
                "x café\n日本語",
            ),
            (
                Event::Paste("replacement\r\ntext".to_string()),
                "replacement\ntext",
            ),
            (
                Event::Key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL)),
                "x café\n日本語",
            ),
            (
                Event::Key(KeyEvent::new(
                    KeyCode::Char('z'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                )),
                "replacement\ntext",
            ),
        ] {
            let mut incoming = Vec::new();
            read_event(
                event,
                &mut incoming,
                &mut pointer,
                &mut screen,
                &mut terminal,
                &app,
            )
            .expect("editing input");
            screen
                .frame(&mut host, &mut router, incoming, (80, 24))
                .expect("editing frame");
            assert_eq!(app.draft.peek(), expected);
            if expected == "replacement\ntext" {
                assert!(
                    !app.pane_focus.peek(),
                    "paste returns Enter to the composer"
                );
            }
            app.refresh_status();
            screen
                .frame(&mut host, &mut router, [], (80, 24))
                .expect("status frame");
        }
    });
}

#[test]
fn paste_and_undo_target_each_open_editor() {
    for editor in ["auth", "model", "workflow", "file"] {
        sigil::reactive::Runtime::scope("summoner-editor-paste", || {
            let (cmd, _rx) = unbounded_channel();
            let app = App::new(info(), PathBuf::from("."), cmd);
            app.resize(80, 24);
            app.replace("unsent prompt".to_string());
            let value = match editor {
                "auth" => {
                    app.auth_open.set(true);
                    app.auth_entering.set(true);
                    app.auth_key
                }
                "model" => {
                    app.editing_model.set(true);
                    app.model_draft
                }
                "workflow" => {
                    app.workflow_open.set(true);
                    app.workflow_naming.set(true);
                    app.workflow_draft
                }
                _ => {
                    app.splash.set(false);
                    app.view_edit.set(true);
                    app.view_text
                }
            };
            let mut terminal = ManuallyDrop::new(Terminal {
                paste: Default::default(),
                out: BufWriter::new(stdout()),
                focused: true,
                redraw: false,
                anchor: None,
                selection: None,
            });
            sigil_tui::widgets::install(super::Paste {
                text: terminal.paste.clone(),
                copy: app.copy,
            });
            let tree = app.clone();
            let mut host = Host::cells();
            host.mount(move |cx: &mut Cx| cx.add(root(tree)));
            let mut router = EventRouter::new();
            let mut screen = Screen::new(80, 24);
            screen
                .frame(&mut host, &mut router, [], (80, 24))
                .expect("mount editor");
            router.focus_next(host.cx());
            let mut pointer = Point::new(0.0, 0.0);
            for (event, expected) in [
                (
                    Event::Paste("café 日本語\r\nsecond line".to_string()),
                    "café 日本語\nsecond line",
                ),
                (
                    Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
                    "café 日本語\nsecond line",
                ),
                (Event::Paste("replacement".to_string()), "replacement"),
                (
                    Event::Key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL)),
                    "café 日本語\nsecond line",
                ),
            ] {
                let mut incoming = Vec::new();
                read_event(
                    event,
                    &mut incoming,
                    &mut pointer,
                    &mut screen,
                    &mut terminal,
                    &app,
                )
                .expect("input");
                screen
                    .frame(&mut host, &mut router, incoming, (80, 24))
                    .expect("editor frame");
                assert_eq!(value.peek(), expected, "{editor}");
                assert_eq!(app.draft.peek(), "unsent prompt", "{editor}");
            }
        });
    }
}

#[test]
fn wheel_scrolls_the_pane_under_the_pointer_without_moving_chat() {
    use crate::tui::Msg;
    use sigil_ide_core::buffer::Rope;
    sigil::reactive::Runtime::scope("summoner-pane-wheel", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.splash.set(false);
        app.resize(120, 40);
        app.view_open("notes.md".to_string());
        app.update(Msg::View {
            path: "notes.md".to_string(),
            file: PathBuf::from("notes.md"),
            rope: Rope::from_str(&"file line\n".repeat(120)),
            added: Vec::new(),
            image: None,
        });
        app.focus_pane(false);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        screen
            .frame(&mut host, &mut router, [], (120, 40))
            .expect("mount panes");
        let chat = app.scroll_y.peek();
        let mut pointer = Point::new(0.0, 0.0);
        for kind in [MouseEventKind::ScrollDown, MouseEventKind::ScrollUp] {
            let mouse = MouseEvent {
                kind,
                column: 110,
                row: 10,
                modifiers: KeyModifiers::NONE,
            };
            let incoming = sigil_tui::map_mouse(&mouse, Host::CELL, &mut pointer);
            screen
                .frame(&mut host, &mut router, incoming, (120, 40))
                .expect("wheel frame");
            assert_eq!(
                app.view_scroll.peek(),
                if kind == MouseEventKind::ScrollDown {
                    3
                } else {
                    0
                }
            );
            assert_eq!(app.scroll_y.peek(), chat);
            assert!(!app.pane_focus.peek());
        }
    });
}

#[cfg(windows)]
use std::{
    os::windows::io::AsRawHandle,
    sync::{Mutex, MutexGuard},
};
#[cfg(windows)]
use windows_sys::Win32::System::Console::{AllocConsole, ENABLE_MOUSE_INPUT, GetConsoleMode};

#[cfg(windows)]
#[expect(unsafe_code, reason = "Test reads the real Windows console mode")]
fn mode(input: &File) -> u32 {
    let mut mode = 0;
    // SAFETY: The file owns a live console handle and mode is writable.
    assert_ne!(
        unsafe { GetConsoleMode(input.as_raw_handle(), &mut mode) },
        0
    );
    mode
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Headless tests need a Windows console for native mouse capture"
)]
fn console() -> (MutexGuard<'static, ()>, File, u32) {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().expect("console fixture lock");
    let open = || File::options().read(true).write(true).open("CONIN$");
    let input = open().unwrap_or_else(|_| {
        // SAFETY: AllocConsole has no pointer arguments and attaches only this test process.
        assert_ne!(unsafe { AllocConsole() }, 0);
        open().expect("console input")
    });
    let before = mode(&input);
    (guard, input, before)
}

#[test]
fn clean_quit_wins_channel_close_race() {
    assert_eq!(status(true, true), Some(0));
    assert_eq!(status(true, false), Some(1));
    assert_eq!(status(false, false), None);
}

#[test]
fn selection_returns_mouse_control_to_the_terminal() {
    let mut bytes = Vec::new();
    #[cfg(windows)]
    let (_guard, input, before) = console();
    #[cfg(windows)]
    mouse(&mut bytes, false).expect("initialize mouse capture");
    mouse(&mut bytes, true).unwrap_or_else(|error| panic!("disable mouse: {error}"));
    #[cfg(windows)]
    assert_eq!(mode(&input), before);
    #[cfg(unix)]
    {
        let disabled = String::from_utf8_lossy(&bytes);
        assert!(disabled.contains("\x1b[?1000l"), "{disabled:?}");
    }

    bytes.clear();
    mouse(&mut bytes, false).unwrap_or_else(|error| panic!("enable mouse: {error}"));
    #[cfg(windows)]
    {
        assert_ne!(mode(&input) & ENABLE_MOUSE_INPUT, 0);
        mouse(&mut bytes, true).expect("restore mouse capture");
    }
    #[cfg(unix)]
    {
        let enabled = String::from_utf8_lossy(&bytes);
        assert!(enabled.contains("\x1b[?1000h"), "{enabled:?}");
    }
}

#[test]
fn restoration_ends_synchronized_output_and_terminal_modes() {
    let mut bytes = Vec::new();
    #[cfg(windows)]
    let (_guard, input, before) = console();
    #[cfg(windows)]
    mouse(&mut bytes, false).expect("initialize mouse capture");
    restore(&mut bytes).unwrap_or_else(|error| panic!("{error}"));
    #[cfg(windows)]
    assert_eq!(mode(&input), before);
    let output = String::from_utf8_lossy(&bytes);

    assert!(output.contains("\x1b[?2026l"), "{output:?}");
    assert!(output.contains("\x1b[?2004l"), "{output:?}");
    assert!(output.contains("\x1b[?25h"), "{output:?}");
    assert!(output.contains("\x1b[?1049l"), "{output:?}");
}
use std::env::var_os;

#[test]
fn the_title_names_the_workspace_and_the_state() {
    sigil::reactive::Runtime::scope("mage-title-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        assert_eq!(
            super::title(&app, true),
            "summon · mage-view-test · working"
        );
        assert_eq!(super::title(&app, false), "summon · mage-view-test · ready");
    });
}
