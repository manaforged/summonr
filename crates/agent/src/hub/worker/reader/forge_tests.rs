use std::path::{Path, PathBuf};

use super::Reader;
use crate::forge::{Ask, Failure, State};
use crate::hub::{Cmd, Fact, Msg};
use crate::worktree::tests::{git, repo};

const FAKE: &str = "SUMMONR_TEST_FAKE_GH";
const SIGNED_IN: &str = r#"{"hosts":{"github.com":[{"state":"success","active":true,"host":"github.com","login":"dev"}]}}"#;
const SIGNED_OUT: &str = r#"{"hosts":{}}"#;
const URL: &str = "https://github.com/owner/repo.git";

fn with_fake_gh(test: &str) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(FAKE) {
        return Some(PathBuf::from(dir));
    }
    let fake = tempfile::tempdir().unwrap();
    let bin = fake.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    summonr_core::dirs::link_fixture("gh", &bin.join("gh"));
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, rest)| rest);
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([&format!("{module}::{test}"), "--exact", "--test-threads=1"])
        .env(FAKE, fake.path())
        .env("PATH", path)
        .status()
        .unwrap();
    assert!(status.success(), "{test} failed with the fake gh");
    None
}

fn github_repo(fake: &Path, auth: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    std::fs::write(fake.join("auth.json"), auth).unwrap();
    let (root, primary) = repo();
    let bare = root.path().join("remote.git");
    git(root.path(), &["init", "-q", "--bare", "remote.git"]);
    git(&primary, &["config", "commit.gpgsign", "false"]);
    git(&primary, &["remote", "add", "origin", URL]);
    let rewrite = format!("url.{}.insteadOf", bare.display());
    git(&primary, &["config", &rewrite, URL]);
    git(&primary, &["push", "-q", "origin", "main"]);
    (root, primary, bare)
}

async fn press(dir: &Path, ask: Ask) -> Result<Option<crate::forge::Pull>, Failure> {
    let (ui, mut inbox) = tokio::sync::mpsc::unbounded_channel();
    let reader = Reader::new(dir.to_path_buf(), Default::default(), ui, true);
    reader.run(Cmd::Forge(ask)).await;
    while let Some(msg) = inbox.recv().await {
        if let Msg::Fact(Fact::Forge(reply)) = msg {
            return reply;
        }
    }
    panic!("the hub sent no forge reply");
}

fn read(dir: &Path, args: &[&str]) -> String {
    crate::diff::git_env(dir, &[], args).unwrap()
}

fn calls(fake: &Path) -> Vec<String> {
    std::fs::read_to_string(fake.join("argv.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn open_pr_on_the_default_branch_moves_the_work_and_a_second_press_finds_the_pr() {
    let Some(fake) = with_fake_gh(
        "open_pr_on_the_default_branch_moves_the_work_and_a_second_press_finds_the_pr",
    ) else {
        return;
    };
    let (_root, primary, bare) = github_repo(&fake, SIGNED_IN);
    let main = read(&primary, &["rev-parse", "main"]);
    std::fs::write(primary.join("parser.rs"), "fn parse() {}\n").unwrap();

    let first = press(&primary, Ask::Open("Fix the parser!".to_owned())).await;

    let pull = first.unwrap().unwrap();
    assert_eq!((pull.number, pull.state), (7, State::Open));
    assert_eq!(read(&primary, &["rev-parse", "main"]), main);
    assert_eq!(
        read(&primary, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "summonr/fix-the-parser"
    );
    assert_eq!(
        read(&bare, &["show", "summonr/fix-the-parser:parser.rs"]),
        "fn parse() {}"
    );
    assert_eq!(read(&primary, &["status", "--porcelain"]), "");

    std::fs::write(primary.join("parser.rs"), "fn parse() -> u8 { 1 }\n").unwrap();
    let second = press(&primary, Ask::Open("Fix the parser!".to_owned())).await;

    assert_eq!(second.unwrap().map(|pull| pull.number), Some(7));
    assert_eq!(
        read(&bare, &["show", "summonr/fix-the-parser:parser.rs"]),
        "fn parse() -> u8 { 1 }"
    );
    assert_eq!(read(&primary, &["rev-parse", "main"]), main);
    let created = calls(&fake)
        .iter()
        .filter(|call| call.starts_with("pr create"))
        .count();
    assert_eq!(created, 1, "{:?}", calls(&fake));
}

#[tokio::test(flavor = "multi_thread")]
async fn open_pr_while_gh_is_signed_out_changes_nothing() {
    let Some(fake) = with_fake_gh("open_pr_while_gh_is_signed_out_changes_nothing") else {
        return;
    };
    let (_root, primary, bare) = github_repo(&fake, SIGNED_OUT);
    let main = read(&primary, &["rev-parse", "main"]);
    std::fs::write(primary.join("parser.rs"), "fn parse() {}\n").unwrap();
    let dirty = read(&primary, &["status", "--porcelain"]);

    let reply = press(&primary, Ask::Open("Fix the parser".to_owned())).await;

    assert_eq!(reply, Err(Failure::SignedOut));
    assert_eq!(reply.unwrap_err().to_string(), "GitHub not signed in");
    assert_eq!(
        read(&primary, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "main"
    );
    assert_eq!(read(&primary, &["rev-parse", "main"]), main);
    assert_eq!(read(&primary, &["status", "--porcelain"]), dirty);
    assert_eq!(
        read(
            &bare,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"]
        ),
        "main"
    );
    assert_eq!(calls(&fake), ["auth status --json hosts"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_gh_call_past_its_limit_is_a_timeout_not_a_hang() {
    let Some(fake) = with_fake_gh("a_gh_call_past_its_limit_is_a_timeout_not_a_hang") else {
        return;
    };
    let (_root, primary, _bare) = github_repo(&fake, SIGNED_IN);
    std::fs::write(fake.join("stall"), "").unwrap();
    let asked = std::time::Instant::now();

    let reply = press(&primary, Ask::Status).await;

    let secs = crate::limits::limits().gh().as_secs();
    let error = reply.unwrap_err();
    assert_eq!(
        error,
        Failure::TimedOut {
            command: "pr list".to_owned(),
            secs
        }
    );
    assert_eq!(
        error.to_string(),
        format!("gh pr list did not answer in {secs}s")
    );
    assert!(
        asked.elapsed() < std::time::Duration::from_secs(secs + 5),
        "{:?}",
        asked.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn open_pr_picks_the_next_free_branch_name_when_the_slug_is_taken() {
    let Some(fake) = with_fake_gh("open_pr_picks_the_next_free_branch_name_when_the_slug_is_taken")
    else {
        return;
    };
    let (_root, primary, bare) = github_repo(&fake, SIGNED_IN);
    let main = read(&primary, &["rev-parse", "main"]);
    git(&primary, &["branch", "summonr/fix-the-parser"]);
    git(
        &primary,
        &[
            "push",
            "-q",
            "origin",
            "main:refs/heads/summonr/fix-the-parser-2",
        ],
    );
    std::fs::write(primary.join("parser.rs"), "fn parse() {}\n").unwrap();

    let pull = press(&primary, Ask::Open("Fix the parser!".to_owned())).await;

    assert_eq!(pull.unwrap().map(|pull| pull.number), Some(7));
    assert_eq!(
        read(&primary, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "summonr/fix-the-parser-3"
    );
    assert_eq!(
        read(&bare, &["show", "summonr/fix-the-parser-3:parser.rs"]),
        "fn parse() {}"
    );
    assert_eq!(
        read(&primary, &["rev-parse", "summonr/fix-the-parser"]),
        main
    );
    assert_eq!(
        read(&bare, &["rev-parse", "summonr/fix-the-parser-2"]),
        main
    );
}
