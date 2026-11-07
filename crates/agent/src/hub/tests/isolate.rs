use super::*;

fn shown(msg: &Msg, count: usize) -> Option<crate::hub::Live> {
    let Msg::Fact(Fact::Live(live)) = msg else {
        return None;
    };
    (live.len() == count)
        .then(|| live.iter().find(|entry| entry.active).cloned())
        .flatten()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_topic_in_a_repo_gets_its_own_branch_and_leaves_the_first_alone() {
    let home = tempfile::tempdir().unwrap();
    let (_root, primary) = crate::worktree::tests::repo();
    let primary = primary.canonicalize().unwrap();
    let first = session(home.path(), &primary);
    let made_home = home.path().to_path_buf();
    let factory: crate::hub::Factory = std::sync::Arc::new(move |open, _| {
        let folder = match open {
            crate::hub::Open::Worktree(folder) => crate::worktree::add(&folder)?.path,
            crate::hub::Open::Folder(folder) => folder,
            other => panic!("a topic opens a folder, not {other:?}"),
        };
        Ok((session(&made_home, &folder), None))
    });
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let started = crate::hub::start(first, info(), (tx, rx), factory);
    let mut inbox = started.inbox;
    let send = |command| started.commands.send(command).unwrap();
    let new_topic = || crate::hub::Cmd::Open(crate::hub::Open::Folder(primary.clone()));
    send(new_topic());
    let one = until("the first topic", &mut inbox, |msg| shown(msg, 2)).await;
    assert_eq!(one.workspace, primary);
    send(crate::hub::Cmd::Command("/name first topic".to_owned()));
    until("the first topic named", &mut inbox, |msg| {
        shown(msg, 2).filter(|entry| entry.title == "first topic")
    })
    .await;
    std::fs::write(primary.join("README"), "first topic's work").unwrap();
    send(new_topic());
    let two = until("the second topic", &mut inbox, |msg| shown(msg, 3)).await;
    assert_ne!(two.workspace, primary);
    assert_eq!(
        crate::worktree::checkout_of(&two.workspace),
        Some(primary.clone())
    );
    let branch =
        crate::diff::git_env(&two.workspace, &[], &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap();
    assert!(branch.starts_with("summonr/"), "{branch}");
    assert_eq!(
        std::fs::read_to_string(two.workspace.join("README")).unwrap(),
        "hi"
    );
    assert_eq!(
        std::fs::read_to_string(primary.join("README")).unwrap(),
        "first topic's work"
    );
    send(crate::hub::Cmd::Quit);
    started.task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn marking_an_idle_agent_unread_shows_in_the_roster() {
    let home = tempfile::tempdir().unwrap();
    let work = home.path().to_path_buf();
    let first = session(home.path(), &work);
    let factory: crate::hub::Factory = std::sync::Arc::new(|_, _| {
        Err(summonr_core::error::Error::Config(
            "no second agent".to_owned(),
        ))
    });
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let started = crate::hub::start(first, info(), (tx, rx), factory);
    let mut inbox = started.inbox;
    let id = until("the first roster", &mut inbox, |msg| {
        let Msg::Fact(Fact::Live(live)) = msg else {
            return None;
        };
        live.first().map(|entry| entry.session.clone())
    })
    .await;
    started
        .commands
        .send(crate::hub::Cmd::MarkUnread(id))
        .unwrap();
    until("the unread mark", &mut inbox, |msg| {
        let Msg::Fact(Fact::Live(live)) = msg else {
            return None;
        };
        live.iter()
            .any(|entry| entry.status == Status::Review)
            .then_some(())
    })
    .await;
    started.commands.send(crate::hub::Cmd::Quit).unwrap();
    started.task.await.unwrap();
}
