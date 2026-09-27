use sigil::Host;
use sigil::events::EventRouter;
use sigil::prelude::Cx;
use sigil::reactive::Runtime;
use sigil::terminal::Screen;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::super::view::root;
use super::busy_info;

fn app(workspace: &std::path::Path) -> App {
    std::fs::create_dir(workspace.join(".git")).unwrap();
    std::fs::write(workspace.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let (commands, _receiver) = unbounded_channel();
    let app = App::new(busy_info(), workspace.to_owned(), commands);
    app.resize(100, 30);
    app
}

fn render(host: &mut Host, screen: &mut Screen, router: &mut EventRouter) -> String {
    for _ in 0..2 {
        screen.frame(host, router, [], (100, 30)).unwrap();
    }
    screen
        .frame(host, router, [], (100, 30))
        .unwrap()
        .text()
        .to_string()
}

#[test]
fn the_credential_dialog_shows_each_typed_character() {
    Runtime::scope("secret-typing", || {
        let workspace = tempfile::tempdir().unwrap();
        let app = app(workspace.path());
        app.set_secret(Some(("Password for git:".to_string(), 0)));
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut screen = Screen::new(100, 30);
        let mut router = EventRouter::new();
        drop(render(&mut host, &mut screen, &mut router));
        app.set_secret(Some(("Password for git:".to_string(), 5)));
        let shown = render(&mut host, &mut screen, &mut router);
        assert_eq!(shown.matches('•').count(), 5, "{shown}");
    });
}

#[test]
fn closing_the_credential_dialog_cancels_the_request() {
    Runtime::scope("secret-close", || {
        let workspace = tempfile::tempdir().unwrap();
        let app = app(workspace.path());
        let (request, answer) = crate::askpass::request("Password for git:");
        app.secret_open(request);
        app.secret_dialog.set(false);
        assert!(matches!(answer.try_recv(), Ok(None)));
        assert!(!app.secret_active());
    });
}
