use std::path::{Path, PathBuf};
use std::process::Command;

use sigil::prelude::Role;

use super::harness::{Step, graded, steps};
use super::in_window;
use summonr_agent::hub::{Cmd, Fact, Msg, Open, Snapshot};
use summonr_agent::session::SessionInfo;

const BRANCH: &str = "tokyo-lights";

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Dev")
        .env("GIT_AUTHOR_EMAIL", "dev@example.com")
        .env("GIT_COMMITTER_NAME", "Dev")
        .env("GIT_COMMITTER_EMAIL", "dev@example.com")
        .output()
        .expect("run fixture git");
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Shop {
    _root: tempfile::TempDir,
    repo: PathBuf,
    tree: PathBuf,
}

fn shop_with_a_conductor_workspace() -> Shop {
    let root = tempfile::tempdir().expect("root");
    let repo = root.path().join("shop");
    std::fs::create_dir_all(&repo).expect("repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README"), "hi\n").expect("readme");
    git(&repo, &["add", "README"]);
    git(&repo, &["commit", "-qm", "init"]);
    let tree = root.path().join("conductor/workspaces/shop/tokyo");
    std::fs::create_dir_all(tree.parent().expect("parent")).expect("workspaces");
    let target = tree.to_str().expect("utf-8 path");
    git(&repo, &["worktree", "add", "-q", "-b", BRANCH, target]);
    std::fs::write(tree.join("README"), "half-done edit\n").expect("dirty edit");
    std::fs::write(tree.join("notes.txt"), "scratch\n").expect("untracked file");
    Shop {
        repo: repo.canonicalize().expect("repo path"),
        tree: tree.canonicalize().expect("tree path"),
        _root: root,
    }
}

fn snapshot(tree: &Path) -> (String, String, String) {
    (
        git(tree, &["status", "--porcelain"]),
        std::fs::read_to_string(tree.join("README")).expect("readme"),
        std::fs::read_to_string(tree.join("notes.txt")).expect("notes"),
    )
}

fn open_in(workspace: PathBuf, chats: Vec<(SessionInfo, bool)>) -> impl FnOnce(&super::super::App) {
    move |app| {
        app.window.sidebar.set(true);
        app.update(Msg::Fact(Fact::Settings(
            Snapshot {
                workspace,
                ..graded()
            }
            .into(),
        )));
        app.update(Msg::Fact(Fact::Sessions(Ok(chats))));
    }
}

fn rows(controls: &[(Role, String, bool)]) -> Vec<&str> {
    controls
        .iter()
        .filter(|(role, _, _)| *role == Role::ListItem)
        .map(|(_, name, _)| name.as_str())
        .collect()
}

#[test]
fn a_conductor_workspace_is_a_topic_and_opening_it_leaves_its_files_alone() {
    if !in_window(
        "worktrees::a_conductor_workspace_is_a_topic_and_opening_it_leaves_its_files_alone",
    ) {
        return;
    }
    let shop = shop_with_a_conductor_workspace();
    let before = snapshot(&shop.tree);
    let seen = steps(
        open_in(shop.repo.clone(), Vec::new()),
        vec![Step::Click(BRANCH)],
        |_| (),
    );
    let listed = rows(&seen.controls);
    assert!(listed.contains(&BRANCH), "{listed:?}");
    assert!(!listed.contains(&"tokyo"), "{listed:?}");
    let opened: Vec<&Open> = seen
        .sent
        .iter()
        .filter_map(|cmd| match cmd {
            Cmd::Open(open) => Some(open),
            _ => None,
        })
        .collect();
    assert_eq!(opened, vec![&Open::Folder(shop.tree.clone())]);
    assert_eq!(snapshot(&shop.tree), before);
}

#[test]
fn an_agent_in_a_worktree_lists_under_its_branch() {
    if !in_window("worktrees::an_agent_in_a_worktree_lists_under_its_branch") {
        return;
    }
    let shop = shop_with_a_conductor_workspace();
    let chat = SessionInfo {
        id: "01a1000000000000000000000000aaaa".to_owned(),
        path: PathBuf::new(),
        cwd: Some(shop.tree.display().to_string()),
        model: None,
        preview: "Ship the lights".to_owned(),
        name: None,
        parent: None,
        depth: 0,
        interactive: true,
        agent: None,
        topic: None,
        modified: None,
        format: summonr_agent::session::FORMAT,
    };
    let seen = steps(
        open_in(shop.tree.clone(), vec![(chat, false)]),
        Vec::new(),
        |_| (),
    );
    let listed = rows(&seen.controls);
    assert!(!listed.contains(&"tokyo"), "{listed:?}");
    assert_eq!(
        after(&listed, "shop", 2),
        vec![BRANCH, "Ship the lights"],
        "{listed:?}"
    );
}

#[test]
fn a_terminal_in_a_worktree_lists_under_its_branch() {
    if !in_window("worktrees::a_terminal_in_a_worktree_lists_under_its_branch") {
        return;
    }
    let _runtime = super::harness::pumped();
    let shop = shop_with_a_conductor_workspace();
    let seen = steps(
        open_in(shop.tree.clone(), Vec::new()),
        vec![Step::Key(super::harness::command("t"))],
        |_| (),
    );
    let listed = rows(&seen.controls);
    assert!(!listed.contains(&"tokyo"), "{listed:?}");
    assert_eq!(
        after(&listed, "shop", 2),
        vec![BRANCH, "Terminal"],
        "{listed:?}"
    );
}

fn after<'a>(listed: &[&'a str], row: &str, count: usize) -> Vec<&'a str> {
    let at = listed
        .iter()
        .position(|seen| *seen == row)
        .map_or(listed.len(), |at| at + 1);
    listed.iter().skip(at).take(count).copied().collect()
}

#[test]
fn a_leftover_run_worktree_is_not_listed() {
    if !in_window("worktrees::a_leftover_run_worktree_is_not_listed") {
        return;
    }
    let shop = shop_with_a_conductor_workspace();
    let run = shop
        .repo
        .parent()
        .expect("root")
        .join("shop-worktrees/18e5d8d1791d1038");
    let target = run.to_str().expect("utf-8 path");
    git(
        &shop.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "summonr/18e5d8d1791d1038",
            target,
        ],
    );
    let seen = steps(open_in(shop.repo.clone(), Vec::new()), Vec::new(), |_| ());
    let listed = rows(&seen.controls);
    assert!(listed.contains(&BRANCH), "{listed:?}");
    assert!(
        !listed.iter().any(|row| row.contains("18e5d8d1791d1038")),
        "{listed:?}"
    );
}
