use super::*;

fn shown(msg: &Msg, count: usize) -> Option<crate::hub::Live> {
    let Msg::Fact(Fact::Live(live)) = msg else {
        return None;
    };
    (live.len() == count)
        .then(|| live.iter().find(|entry| entry.active).cloned())
        .flatten()
}

fn branch(folder: &std::path::Path) -> String {
    crate::diff::git_env(folder, &[], &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn two_new_topics_plus_the_checkout_are_three_folders_on_three_branches() {
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
    let new_topic = |folder: &std::path::Path| {
        crate::hub::Cmd::Open(crate::hub::Open::Worktree(folder.to_path_buf()))
    };
    std::fs::write(primary.join("README"), "the checkout's work").unwrap();
    send(new_topic(&primary));
    let one = until("the first new topic", &mut inbox, |msg| shown(msg, 2)).await;
    send(crate::hub::Cmd::Command("/name first topic".to_owned()));
    until("the first topic named", &mut inbox, |msg| {
        shown(msg, 2).filter(|entry| entry.title == "first topic")
    })
    .await;
    send(new_topic(&one.workspace));
    let two = until("the second new topic", &mut inbox, |msg| shown(msg, 3)).await;
    let folders = [
        primary.clone(),
        one.workspace.clone(),
        two.workspace.clone(),
    ];
    let branches = folders.each_ref().map(|folder| branch(folder));
    assert_eq!(
        folders
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3,
        "{folders:?}"
    );
    assert_eq!(
        branches
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3,
        "{branches:?}"
    );
    assert!(
        branches[1..]
            .iter()
            .all(|name| name.starts_with("summonr/")),
        "{branches:?}"
    );
    for topic in &folders[1..] {
        assert_eq!(crate::worktree::checkout_of(topic), Some(primary.clone()));
        assert_eq!(std::fs::read_to_string(topic.join("README")).unwrap(), "hi");
    }
    send(crate::hub::Cmd::Open(crate::hub::Open::Folder(
        primary.clone(),
    )));
    let opened = until("the opened folder", &mut inbox, |msg| shown(msg, 4)).await;
    assert_eq!(opened.workspace, primary);
    assert_eq!(
        std::fs::read_to_string(primary.join("README")).unwrap(),
        "the checkout's work"
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
