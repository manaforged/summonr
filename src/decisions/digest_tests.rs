use super::*;

const TRANSCRIPT: &str = concat!(
    "# session abc123 · glm-5.3 · /home/t/repo\n",
    "\n## You\n\nFix the failing build and do not touch the UI.\n",
    "\n## Assistant\n\nLooking now.\n",
    "- `run_command` {\"command\": \"cargo check --locked\"}\n",
    "```\nexit 1\nerror[E0004]: non-exhaustive patterns: `Wire::Decisions` not covered\n```\n",
    "- `read` {\"path\": \"src/a.rs\"}\n",
    "- `edit` {\"path\": \"src/a.rs\"}\n",
    "- `run_command` {\"command\": \"cargo check --locked\"}\n",
    "```\nexit 1\nerror[E0004]: non-exhaustive patterns: `Wire::Decisions` not covered\n```\n",
    "- `run_command` {\"command\": \"cargo check --locked\"}\n",
    "```\nexit 0\n```\n",
    "- `write` {\"path\": \"src/b.rs\"}\n",
    "\n## Assistant\n\nThe build passes now.\n",
);

#[test]
fn the_digest_keeps_the_commands_and_drops_the_prose() {
    let text = digest(TRANSCRIPT);
    assert!(text.contains("3x cargo check --locked"), "got {text}");
    assert!(
        !text.contains("Looking now"),
        "chatter is not evidence and must not be sent: {text}"
    );
}

#[test]
fn the_digest_is_a_fraction_of_the_transcript() {
    let padding = "some assistant prose that says very little\n".repeat(500);
    let bloated = TRANSCRIPT.replace("Looking now.", &padding);
    let text = digest(&bloated);
    assert!(
        text.len() * 8 < bloated.len(),
        "digest {} vs transcript {}",
        text.len(),
        bloated.len()
    );
}

#[test]
fn a_repeated_command_is_counted_not_repeated() {
    let text = digest(TRANSCRIPT);
    assert_eq!(
        text.matches("cargo check --locked").count(),
        1,
        "the same command must appear once with a count: {text}"
    );
    assert!(text.contains("3 run, 1 distinct"), "got {text}");
}

#[test]
fn exit_codes_survive_because_they_are_the_outcome() {
    let text = digest(TRANSCRIPT);
    assert!(text.contains("exit 0: 1"), "got {text}");
    assert!(text.contains("exit 1: 2"), "got {text}");
}

#[test]
fn the_first_error_line_is_kept_once() {
    let text = digest(TRANSCRIPT);
    assert_eq!(
        text.matches("error[E0004]").count(),
        1,
        "the same error twice is one fact: {text}"
    );
}

#[test]
fn the_task_and_the_closing_claim_both_survive() {
    let text = digest(TRANSCRIPT);
    assert!(
        text.contains("Fix the failing build"),
        "the task is what the session is judged against: {text}"
    );
    assert!(
        text.contains("The build passes now"),
        "the closing claim is what a reviewer checks: {text}"
    );
}

#[test]
fn edited_paths_are_listed_and_reads_are_only_counted() {
    let text = digest(TRANSCRIPT);
    assert!(text.contains("src/a.rs"), "got {text}");
    assert!(text.contains("src/b.rs"), "got {text}");
    assert!(text.contains("read 1 times"), "got {text}");
    assert!(text.contains("2 distinct paths"), "got {text}");
}

#[test]
fn an_empty_transcript_still_gives_a_shape() {
    let text = digest("");
    assert!(text.contains("commands: 0 run"), "got {text}");
}

#[test]
fn a_command_with_escaped_quotes_is_read_whole() {
    let line = "- `run_command` {\"command\": \"echo \\\"hi there\\\" && ls\"}\n";
    let text = digest(&format!("# session a · m · /r\n{line}"));
    assert!(text.contains("echo \"hi there\" && ls"), "got {text}");
}
