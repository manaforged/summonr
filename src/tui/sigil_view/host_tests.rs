use super::{mouse, restore, status};

#[cfg(windows)]
use std::{
    fs::File,
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
