use super::*;

fn with_user_memory(text: &str) -> (tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join(DIR).join(FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    (home, tempfile::tempdir().unwrap())
}

fn one(text: &str) -> String {
    let (home, work) = with_user_memory(text);
    sections(&files(home.path(), work.path()), work.path()).join("\n")
}

fn project(home: &Path, dir: &Path) -> PathBuf {
    files(home, dir)
        .into_iter()
        .find(|file| file.scope == Scope::Project)
        .unwrap()
        .path
}

#[test]
fn an_oversized_memory_keeps_the_newest_entries_and_says_to_read_first() {
    let cap = crate::limits::limits().memory_bytes;
    let entries: String = (0..2000)
        .map(|n| format!("- 2026-10-{:02} entry {n} é\n", n % 28 + 1))
        .collect();
    let text = one(&entries);
    let body = text
        .split_once('\n')
        .and_then(|(_, rest)| rest.rsplit_once("\n</memory>"))
        .map(|(body, _)| body)
        .unwrap();
    assert!(body.starts_with("["), "{body}");
    assert!(
        body.contains("older bytes of this file are not shown"),
        "{body}"
    );
    assert!(body.contains("Read "), "{body}");
    assert!(body.contains("entry 1999"), "newest entry kept");
    assert!(!body.contains("entry 0 "), "oldest entry dropped");
    assert!(body.len() <= cap, "{} > {cap}", body.len());
}

#[test]
fn a_moved_repository_keeps_its_memory() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let before = root.path().join("api");
    std::fs::create_dir_all(&before).unwrap();
    let git = |dir: &Path, args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .status()
                .unwrap()
                .success()
        );
    };
    git(&before, &["init", "-q"]);
    git(&before, &["commit", "-q", "--allow-empty", "-m", "start"]);
    let first = project(home.path(), &before);
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::write(&first, "- 2026-10-11 deploy on Thursdays\n").unwrap();
    let after = root.path().join("api-renamed");
    std::fs::rename(&before, &after).unwrap();
    assert_eq!(project(home.path(), &after), first);
}

#[test]
fn hidden_characters_and_secrets_never_reach_the_prompt() {
    let text = one(
        "2026-10-11 deploy\u{200B}key\u{202E}\u{061C}\u{00AD} is set\nAPI_TOKEN=placeholder-value-placeholder\n",
    );
    assert!(text.contains("2026-10-11 deploykey is set"), "{text}");
    assert!(!text.contains("placeholder-value"), "{text}");
    assert!(text.contains("API_TOKEN=[secret]"), "{text}");
}

#[test]
fn an_empty_memory_adds_no_section() {
    assert_eq!(one("  \n"), "");
}

#[test]
fn the_memory_report_uses_spaces_and_counts_entries() {
    let (home, work) = with_user_memory("# Memory\n- 2026-10-11 tabs\n2026-10-12 trunk-stable\n");
    let text = report(&files(home.path(), work.path()));
    assert!(!text.contains('\t'), "{text}");
    assert!(text.starts_with("user memory      "), "{text}");
    assert!(text.contains("bytes, 2 entries"), "{text}");
    assert!(text.contains("project memory   "), "{text}");
    assert!(text.contains("\n  empty\n"), "{text}");
}

#[test]
fn look_alike_repository_paths_get_separate_memory() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dash = root.path().join("a-b");
    let under = root.path().join("a_b");
    std::fs::create_dir_all(&dash).unwrap();
    std::fs::create_dir_all(&under).unwrap();
    assert_ne!(project(home.path(), &dash), project(home.path(), &under));
    assert_eq!(project(home.path(), &dash), project(home.path(), &dash));
}

#[test]
fn well_known_token_shapes_are_hidden() {
    let github = format!("gh{}_{}", "p", "A1b2".repeat(9));
    let aws = format!("AK{}{}", "IA", "QWERTYUIOP123456");
    let anthropic = format!("sk-{}-api03-{}", "ant", "z9".repeat(20));
    let slack = format!("xo{}-{}", "xb", "1234567890-abcdefghij");
    let text = one(&format!(
        "- tokens {github} {aws} {anthropic} {slack} end\n"
    ));
    for secret in [&github, &aws, &anthropic, &slack] {
        assert!(!text.contains(secret.as_str()), "{secret} leaked: {text}");
    }
    assert!(text.contains("[secret]"), "{text}");
}

#[test]
fn guidance_says_an_empty_memory_has_nothing_to_read() {
    let (home, work) = with_user_memory("- 2026-10-13 a stored fact\n");
    let text = guidance(&files(home.path(), work.path()));
    let project = project(home.path(), work.path()).display().to_string();
    assert!(
        text.contains(&format!("{project} (empty, nothing to read)")),
        "{text}"
    );
    let user = home.path().join(DIR).join(FILE).display().to_string();
    assert!(!text.contains(&format!("{user} (empty")), "{text}");
}

#[test]
fn a_torn_first_commit_cache_keeps_the_project_memory() {
    let home = tempfile::tempdir().unwrap();
    let (_root, primary) = crate::worktree::tests::repo();
    let first = project(home.path(), &primary);
    let cached = primary
        .join(".git")
        .join(crate::diff::STATE_DIR)
        .join(FIRST_COMMIT);
    std::fs::write(&cached, "abc\n").unwrap();
    assert_eq!(project(home.path(), &primary), first);
    let id = std::fs::read_to_string(&cached).unwrap();
    assert!(object_id(id.trim()), "{id:?}");
}

#[test]
fn a_subfolder_of_a_repository_shares_the_repository_memory_root() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let sub = repo.join("nested");
    std::fs::create_dir_all(&sub).unwrap();
    crate::worktree::tests::git(&repo, &["init", "-q"]);
    let top = repo.canonicalize().unwrap();
    assert_eq!(root(&sub, None), top);
    let layout = crate::diff::Layout {
        git_dir: top.join(crate::policy::git::GIT_DIR),
        common: top.join(crate::policy::git::GIT_DIR),
        top: top.clone(),
    };
    assert_eq!(root(&sub, Some(&layout)), top);
}
