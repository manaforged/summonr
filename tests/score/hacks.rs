use serde_json::json;

use super::{Workspace, counter, ran, run_goal, said, todo};

fn configure(workspace: &Workspace, check: &str) {
    std::fs::write(workspace.work.path().join(".summonr/check.toml"), check).unwrap();
}

fn parked(stdout: &str) -> bool {
    stdout.contains("no new measurement in 6 goal turns")
}

#[test]
fn a_score_rerun_with_no_code_change_is_not_progress_and_the_goal_parks() {
    let workspace = Workspace::new("");
    configure(
        &workspace,
        &format!(
            "score = \"n=$({}); echo s $((n * n))\"\n\n[targets]\ns = 1000000\n",
            counter(&workspace, "runs")
        ),
    );
    let run = run_goal(&workspace, "raise s to 1000000", "", Vec::new());
    assert_ne!(run.code, Some(0), "{}", run.stderr);
    assert!(parked(&run.stdout), "{}", run.stdout);
}

#[test]
fn a_score_that_moves_within_its_measured_noise_after_a_code_change_is_not_progress() {
    let workspace = Workspace::new("");
    configure(
        &workspace,
        &format!(
            "score = \"n=$({}); echo s $((10 + n % 2))\"\n\n[targets]\ns = 100\n",
            counter(&workspace, "runs")
        ),
    );
    let mut turns = vec![said("measured"), said("measured again")];
    turns.extend((1..=12).flat_map(|turn| {
        [
            ran(&format!("echo {turn} > scratch.txt")),
            said("measured after a change"),
        ]
    }));
    let run = run_goal(&workspace, "raise s to 100", "", turns);
    assert!(parked(&run.stdout), "{}", run.stdout);
    let edits: u32 = std::fs::read_to_string(workspace.work.path().join("scratch.txt"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(edits <= 6, "the goal ran {edits} changes inside the noise");
}

#[test]
fn a_check_script_edited_to_pass_is_put_back_and_the_goal_does_not_pass() {
    let workspace = Workspace::new("check = \"./check.sh\"\n");
    let script = workspace.work.path().join("check.sh");
    std::fs::write(&script, "#!/bin/sh\ntest -f done.txt\n").unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let run = run_goal(
        &workspace,
        "create done.txt",
        "",
        vec![
            ran("printf '#!/bin/sh\\ntrue\\n' > check.sh"),
            said("the check passes now"),
        ],
    );
    assert_ne!(run.code, Some(0), "{}", run.stdout);
    assert_eq!(
        std::fs::read_to_string(&script).unwrap(),
        "#!/bin/sh\ntest -f done.txt\n"
    );
    assert!(run.stdout.contains("put back check.sh"), "{}", run.stdout);
}

#[test]
fn a_protected_test_edited_to_pass_is_put_back() {
    let workspace = Workspace::new(
        "check = \"for test in tests/*.sh; do sh $test || exit 1; done\"\nprotect = [\"tests\"]\n",
    );
    std::fs::create_dir_all(workspace.work.path().join("tests")).unwrap();
    std::fs::write(
        workspace.work.path().join("tests/done.sh"),
        "test -f done.txt\n",
    )
    .unwrap();
    let run = run_goal(
        &workspace,
        "create done.txt",
        "",
        vec![
            ran("echo true > tests/done.sh"),
            said("the test passes now"),
        ],
    );
    assert_ne!(run.code, Some(0), "{}", run.stdout);
    assert_eq!(
        std::fs::read_to_string(workspace.work.path().join("tests/done.sh")).unwrap(),
        "test -f done.txt\n"
    );
}

#[test]
fn busywork_that_never_changes_the_check_output_parks_a_check_only_goal() {
    let workspace = Workspace::new("check = \"test -f done.txt\"\n");
    let mut turns = Vec::new();
    for (turn, task) in ["read the code", "write notes", "review the notes"]
        .into_iter()
        .enumerate()
    {
        turns.push(ran(&format!("echo {turn} > notes.txt")));
        turns.push(todo(json!([
            {"text": task, "status": "done"},
            {"text": "create done.txt", "status": "pending"},
        ])));
        turns.push(said("finished a task"));
    }
    let run = run_goal(&workspace, "create done.txt", "", turns);
    assert_ne!(run.code, Some(0), "{}", run.stdout);
    assert!(parked(&run.stdout), "{}", run.stdout);
}

#[test]
fn a_goal_that_cannot_be_met_stops_when_its_task_is_marked_blocked() {
    let workspace = Workspace::new(
        "check = \"test \\\"$(cat answer)\\\" = 1 && test \\\"$(cat answer)\\\" = 2\"\n",
    );
    let run = run_goal(
        &workspace,
        "make answer both 1 and 2",
        "",
        vec![
            todo(json!([{"text": "make answer both 1 and 2", "status": "blocked"}])),
            said("answer cannot be 1 and 2 at once"),
        ],
    );
    assert_ne!(run.code, Some(0), "{}", run.stdout);
    assert!(
        run.stdout.contains("blocked: make answer both 1 and 2"),
        "{}",
        run.stdout
    );
    assert!(run.seconds < 60, "the blocked goal ran {}s", run.seconds);
}
