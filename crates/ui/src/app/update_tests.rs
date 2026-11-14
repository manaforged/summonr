use super::*;
use crate::profile::info;
use sigil::prelude::effect;
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn transcript_replay_notifies_model_subscribers_once() {
    sigil::reactive::Owner::scope("transcript-batch", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(info(), workspace.path().to_owned(), commands);
        let runs = Rc::new(Cell::new(0_usize));
        let counter = Rc::clone(&runs);
        let model = app.model;
        effect(move || {
            model.with(|_| ());
            counter.set(counter.get() + 1);
        });
        assert_eq!(runs.get(), 1);
        let events = (0..50)
            .map(|at| summonr_agent::Event::User {
                text: format!("turn {at}"),
                images: Vec::new(),
            })
            .collect();
        app.update(Msg::Fact(Fact::Transcript {
            replace: false,
            events,
        }));
        assert_eq!(runs.get(), 2);
        assert_eq!(app.model.with(|model| model.entries.len()), 50);
    });
}

#[test]
fn app_clone_shares_info_rc() {
    sigil::reactive::Owner::scope("app-info-rc", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let mut details = info();
        details.config = Some(workspace.path().join("config.toml"));
        let app = App::new(details, workspace.path().to_owned(), commands);
        let cloned = app.clone();
        assert!(Rc::ptr_eq(&app.info, &cloned.info));
        assert_eq!(cloned.info.config, app.info.config);
    });
}
