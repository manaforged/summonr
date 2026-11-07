use super::{call, parent};
use crate::provider::scripted::Reply;

fn git(dir: &std::path::Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git runs")
        .success();
    assert!(ok, "git {args:?}");
}

fn names(stats: &[crate::diff::Stat]) -> Vec<&str> {
    stats.iter().map(|stat| stat.path.as_str()).collect()
}

#[tokio::test]
async fn a_shared_childs_delta_is_its_own_files_frozen_when_it_finishes() {
    let child_round = vec![
        call(
            "w",
            "write",
            &serde_json::json!({"path": "a.txt", "content": "child text\n"}),
        ),
        call(
            "r",
            "run_command",
            &serde_json::json!({"command": "echo made > made.txt"}),
        ),
    ];
    let (mut lead, _dirs) = parent(vec![Reply::Calls(child_round), Reply::Text("child done")]);
    lead.tools.set_unrestricted();
    let root = lead.workspace.clone();
    std::fs::write(root.join("a.txt"), "first\n").unwrap();
    std::fs::write(root.join("before.txt"), "committed\n").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "base"]);
    std::fs::write(root.join("before.txt"), "the user's edit\n").unwrap();

    let args = serde_json::from_value(serde_json::json!({
        "assignment": "edit a", "access": "implement", "workspace": "shared"
    }))
    .unwrap();
    let budget = lead.task_budget(1).unwrap();
    let run = lead.task_start_in(args, &budget, None).unwrap();
    std::fs::write(root.join("other.txt"), "a sibling's edit\n").unwrap();
    let (events, _keep) = tokio::sync::mpsc::unbounded_channel();
    let mut done = run.execute(events).await;
    lead.task_record(&mut done).unwrap();

    let records = crate::diff::children(lead.session.path()).unwrap();
    assert_eq!(records.len(), 1, "{records:?}");
    let delta = crate::diff::delta(&root, &records[0]).unwrap();
    assert_eq!(names(&delta.diff.files), ["a.txt", "made.txt"]);
    assert_eq!(names(&delta.diff.strays), ["other.txt"]);
    let patch = delta.patch(None).unwrap();
    assert!(patch.contains("+child text"), "{patch}");
    assert!(!patch.contains("user's edit"), "{patch}");

    std::fs::write(root.join("a.txt"), "the lead's later edit\n").unwrap();
    let later = crate::diff::delta(&root, &records[0]).unwrap();
    assert_eq!(later.patch(None).unwrap(), patch);
}

#[tokio::test]
async fn an_isolated_childs_delta_is_what_landed_in_the_lead_checkout() {
    let task = call(
        "t",
        "task",
        &serde_json::json!({"assignment": "add b", "access": "implement"}),
    );
    let write = call(
        "w",
        "write",
        &serde_json::json!({"path": "b.txt", "content": "landed\n"}),
    );
    let (mut lead, _dirs) = parent(vec![
        Reply::Calls(vec![task]),
        Reply::Calls(vec![write]),
        Reply::Text("child done"),
        Reply::Text("lead done"),
    ]);
    lead.tools.set_unrestricted();
    let root = lead.workspace.clone();
    std::fs::write(root.join("a.txt"), "first\n").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "base"]);

    lead.turn("delegate").await.unwrap();
    assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "landed\n");
    let records = crate::diff::children(lead.session.path()).unwrap();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_ne!(records[0].workspace, root);
    let delta = crate::diff::delta(&root, &records[0]).unwrap();
    assert_eq!(names(&delta.diff.files), ["b.txt"]);
    assert!(delta.diff.strays.is_empty(), "{:?}", delta.diff.strays);
    assert!(delta.patch(None).unwrap().contains("+landed"));
}
