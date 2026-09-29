use super::*;

fn fail(guard: &mut Guard, command: &str, output: &str) {
    let arguments = format!(r#"{{"command":"{command}"}}"#);
    drop(guard.observe("run_command", &arguments, false, output.to_string()));
}

#[test]
fn different_commands_that_fail_silently_do_not_trip_the_guard() {
    let mut guard = Guard::configured(&crate::config::Guard::default());
    for command in [
        "test -f a",
        "test -f b",
        "pgrep x",
        "git diff --quiet",
        "test -d c",
        "cmp -s a b",
        "test -e d",
        "which nope",
        "test -f e",
    ] {
        fail(&mut guard, command, "exit 1");
    }
    assert_eq!(guard.stuck(), None);
}

#[test]
fn one_error_text_from_different_calls_still_trips_the_guard() {
    let mut guard = Guard::configured(&crate::config::Guard::default());
    for attempt in 0..8 {
        fail(
            &mut guard,
            &format!("cargo build -p crate{attempt}"),
            "exit 101\nerror[E0308]: mismatched types in src/lib.rs",
        );
    }
    assert!(guard.stuck().is_some());
}

#[test]
fn a_zero_signature_stop_turns_the_check_off() {
    let config = crate::config::Guard {
        signature_stop: Some(0),
        ..crate::config::Guard::default()
    };
    let mut guard = Guard::configured(&config);
    fail(
        &mut guard,
        "cargo build",
        "exit 101\nerror[E0308]: mismatched types in src/lib.rs",
    );
    assert_eq!(guard.stuck(), None);
}

#[test]
fn polling_a_running_job_never_trips_the_guard() {
    let mut guard = Guard::configured(&crate::config::Guard::default());
    for _ in 0..12 {
        drop(guard.observe(
            "run_command",
            r#"{"job":"j1"}"#,
            true,
            "job j1\nstate: running (120 bytes)".to_string(),
        ));
    }
    assert_eq!(guard.stuck(), None);
}
