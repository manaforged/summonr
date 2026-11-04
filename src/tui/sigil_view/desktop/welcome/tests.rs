use std::cell::RefCell;
use std::rc::Rc;

use sigil::Host;
use sigil::app::FrameHooks;
use sigil::events::{Event, KeyPhase, LogicalKey};
use sigil::prelude::{HostMode, Key, signal};

#[test]
fn a_typed_workspace_path_opens_once_and_waits_while_busy() {
    sigil::reactive::Owner::scope("workspace-path", || {
        let folder = tempfile::tempdir().expect("project folder");
        let path = folder.path().to_string_lossy().into_owned();
        let busy = signal(false);
        let opened = Rc::new(RefCell::new(Vec::new()));
        let captured = opened.clone();
        let open: super::Open = Rc::new(move |path| captured.borrow_mut().push(path));
        let mut host = Host::new(HostMode::Pixel);
        host.mount(move || super::path_input(busy, open));
        frame(&mut host);
        for character in path.chars() {
            host.route(
                Event::key(LogicalKey::character(character.to_string()), KeyPhase::Down)
                    .text(Some(character.to_string())),
            );
            frame(&mut host);
        }
        host.route(Event::key(LogicalKey::named(Key::Enter), KeyPhase::Down));
        frame(&mut host);
        assert_eq!(*opened.borrow(), vec![folder.path().to_path_buf()]);
        busy.set(true);
        frame(&mut host);
        host.route(Event::key(LogicalKey::named(Key::Enter), KeyPhase::Down));
        frame(&mut host);
        assert_eq!(
            opened.borrow().len(),
            1,
            "busy workspace opened a second time"
        );
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn clicking_workspace_search_routes_typing_to_search_not_the_path() {
    sigil::reactive::Owner::scope("workspace-search", || {
        let error = signal(None);
        let busy = signal(false);
        let mut host = Host::new(HostMode::Pixel);
        host.mount(move || super::view(error, busy, |_| {}));
        for _ in 0..4 {
            frame(&mut host);
        }
        let tree = host.accessibility_tree();
        let bounds = tree
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some("Search workspaces"))
                    .then(|| node.bounds())
                    .flatten()
            })
            .expect("search field bounds");
        let point = sigil::prelude::Point::new(
            ((bounds.x0 + bounds.x1) / 2.0) as f32,
            ((bounds.y0 + bounds.y1) / 2.0) as f32,
        );
        host.route(Event::pointer_down(point));
        host.route(Event::pointer_up(point));
        frame(&mut host);
        host.route(Event::key(LogicalKey::character("q"), KeyPhase::Down).text(Some("q".into())));
        frame(&mut host);
        let tree = host.accessibility_tree();
        let search = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Search workspaces"))
            .expect("search field");
        assert_eq!(search.1.value(), Some("q"));
        let path = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                sigil::prelude::Role::from_accesskit(node.role()) == sigil::prelude::Role::TextInput
                    && node.label() != Some("Search workspaces")
            })
            .expect("workspace path field");
        assert!(
            path.1.value().is_none_or(str::is_empty),
            "search typing changed workspace path"
        );
    });
}

fn frame(host: &mut Host) {
    host.frame((1024.0, 700.0), 1.0, FrameHooks::default());
}
