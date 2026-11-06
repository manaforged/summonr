use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn a_new_agent_gets_a_worktree_only_while_another_runs_in_the_checkout() {
    let home = tempfile::tempdir().unwrap();
    let (_root, primary) = crate::worktree::tests::repo();
    let primary = primary.canonicalize().unwrap();
    let first = session(home.path(), &primary);
    let opened = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (seen, made_home, made_work) = (opened.clone(), home.path().to_path_buf(), primary.clone());
    let factory: crate::hub::Factory = std::sync::Arc::new(move |open, _| {
        seen.lock().unwrap().push(open);
        Ok((session(&made_home, &made_work), None))
    });
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let started = crate::hub::start(first, info(), (tx.clone(), rx), factory);
    let mut inbox = started.inbox;
    let open = crate::hub::Open::Folder(primary.clone());
    started
        .commands
        .send(crate::hub::Cmd::Open(open.clone()))
        .unwrap();
    until("the idle open", &mut inbox, |msg| {
        active(msg).filter(|(_, open)| *open == 2).map(|_| ())
    })
    .await;
    tx.send(Msg::Fact(Fact::Busy(Some(Activity::Turn))))
        .unwrap();
    until("the first agent working", &mut inbox, |msg| {
        let Msg::Fact(Fact::Live(live)) = msg else {
            return None;
        };
        live.iter()
            .any(|entry| entry.status == Status::Working)
            .then_some(())
    })
    .await;
    started.commands.send(crate::hub::Cmd::Open(open)).unwrap();
    until("the running open", &mut inbox, |msg| {
        active(msg).filter(|(_, open)| *open == 3).map(|_| ())
    })
    .await;
    assert_eq!(
        *opened.lock().unwrap(),
        vec![
            crate::hub::Open::Folder(primary.clone()),
            crate::hub::Open::Worktree(primary.clone()),
        ]
    );
    started.commands.send(crate::hub::Cmd::Quit).unwrap();
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
