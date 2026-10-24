use serde_json::json;

use super::{Workspace, ran, run_goal, said, todo};

#[test]
fn a_finished_task_starts_the_next_task_in_a_fresh_context() {
    let workspace = Workspace::new("");
    std::fs::write(
        workspace.work.path().join(".summonr/check.toml"),
        "score = \"echo s 1\"\n\n[targets]\ns = 2\n",
    )
    .unwrap();
    let first = todo(
        json!([{"text": "write module A", "status": "done"}, {"text": "write module B", "status": "pending"}]),
    );
    let run = run_goal(
        &workspace,
        "raise s to 2",
        "",
        vec![first, said("module A is written\nit uses the builder")],
    );
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.agent[1].contains("module A is written") || run.agent.len() > 2,
        "{}",
        run.agent.len()
    );
    let second = 2 + run
        .agent
        .iter()
        .skip(2)
        .position(|body| {
            body.contains("Break the objective into tasks") && !body.contains("it uses the builder")
        })
        .unwrap_or_else(|| panic!("no fresh-context turn after the finished task"));
    assert!(second >= 2, "fresh turn at request {second}");
    assert!(
        run.agent[second].contains("write module B"),
        "{}",
        run.agent[second]
    );
}

#[test]
fn a_task_whose_check_output_did_not_change_is_reopened_once_and_keeps_its_context() {
    let workspace = Workspace::new("check = \"echo 0 passed in 0.$$s; test -f done.txt\"\n");
    let marked = || todo(json!([{"text": "create done.txt", "status": "done"}]));
    let run = run_goal(
        &workspace,
        "create done.txt",
        "",
        vec![
            said("I read the objective"),
            marked(),
            said("I marked it done"),
            marked(),
            said("it is done again"),
            ran("touch done.txt"),
            said("done.txt exists now"),
        ],
    );
    assert_eq!(run.code, Some(0), "{}\n{}", run.stderr, run.stdout);
    let reopened =
        "reopened create done.txt: the check output did not change after it was marked done";
    assert_eq!(run.stdout.matches(reopened).count(), 1, "{}", run.stdout);
    let after = run
        .agent
        .iter()
        .position(|body| body.contains("I marked it done"))
        .unwrap_or_else(|| panic!("the reopen turn never reached the model"));
    assert!(
        run.agent[after + 1].contains("I marked it done"),
        "the reopened task kept its context"
    );
}

#[test]
fn each_task_that_moves_a_red_global_check_finishes_and_its_note_reaches_the_next_context() {
    let workspace = Workspace::new("check = \"sh check.sh\"\n");
    std::fs::write(
        workspace.work.path().join("check.sh"),
        "n=$(ls a.txt b.txt c.txt 2>/dev/null | wc -l | tr -d ' ')\necho \"$((3 - n)) failed, $n passed in 0.$$s\"\ntest \"$n\" = 3\n",
    )
    .unwrap();
    let list = |a: &str, b: &str, c: &str| {
        todo(json!([
            {"text": "write a.txt", "status": a},
            {"text": "write b.txt", "status": b},
            {"text": "write c.txt", "status": c},
        ]))
    };
    let run = run_goal(
        &workspace,
        "write a.txt, b.txt, and c.txt",
        "",
        vec![
            list("pending", "pending", "pending"),
            said("planned three tasks"),
            ran("touch a.txt"),
            list("done", "pending", "pending"),
            said("a.txt is written"),
            ran("touch b.txt"),
            list("done", "done", "pending"),
            said("b.txt is written"),
            ran("touch c.txt"),
            list("done", "done", "done"),
            said("c.txt is written"),
        ],
    );
    assert_eq!(run.code, Some(0), "{}\n{}", run.stderr, run.stdout);
    assert!(!run.stdout.contains("reopened"), "{}", run.stdout);
    let fresh = run
        .agent
        .iter()
        .find(|body| {
            body.contains("Done so far")
                && body.contains("- write b.txt: changed b.txt; the check failed")
        })
        .unwrap_or_else(|| panic!("no fresh context carried both notes"));
    assert!(
        fresh.contains("- write a.txt: changed a.txt; the check failed"),
        "{fresh}"
    );
    assert!(!fresh.contains("planned three tasks"), "{fresh}");
}
