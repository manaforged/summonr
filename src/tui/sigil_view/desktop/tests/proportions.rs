use sigil::Host;
use sigil::app::FrameHooks;
use sigil::prelude::{HostMode, Role};
use tokio::sync::mpsc::unbounded_channel;

use super::{App, in_window, root};
use crate::tools::Event;
use crate::tui::{Fact, Msg};

#[test]
fn the_composer_stays_anchored_and_grows_with_the_draft() {
    if !in_window("proportions::the_composer_stays_anchored_and_grows_with_the_draft") {
        return;
    }
    sigil::reactive::Owner::scope("desktop-proportions", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(
            super::super::super::profile::info(),
            workspace.path().into(),
            commands,
        );
        app.replace("Short task".to_owned());
        app.refresh();
        let tree = app.clone();
        let mut host = Host::new(HostMode::Pixel);
        host.mount(move || root(tree.clone()));
        for width in [1024.0, 1280.0, 1920.0, 375.0] {
            app.model_mut(|model| model.entries.clear());
            app.refresh();
            check_width(&app, &mut host, width);
        }
    });
}

fn check_width(app: &App, host: &mut Host, width: f32) {
    let view = (width, 800.0);
    let empty = editor(host, view);
    assert!(empty.2 <= 704.0, "editor too wide at {width}: {empty:?}");
    assert!(
        empty.0 >= 0.0 && empty.0 + empty.2 <= f64::from(width),
        "editor outside window: {empty:?}"
    );
    app.update(Msg::Fact(Fact::Event(Event::Assistant {
        text: "Ready".to_owned(),
    })));
    let active = editor(host, view);
    assert!(
        (active.1 - empty.1).abs() <= 1.0,
        "first reply moved composer: {empty:?} -> {active:?}"
    );
    app.replace("Line one\nLine two\nLine three\nLine four\nLine five\nLine six".to_owned());
    let long = editor(host, view);
    assert!(
        long.3 > active.3,
        "multiline draft did not grow: {active:?} -> {long:?}"
    );
    assert!(long.3 <= 192.0, "draft covered transcript: {long:?}");
    app.replace("Short task".to_owned());
    let short = editor(host, view);
    assert!(
        (short.3 - active.3).abs() <= 1.0,
        "cleared draft did not shrink: {short:?}"
    );
    println!("width={width}: short={active:?}, multiline={long:?}, cleared={short:?}");
}

fn editor(host: &mut Host, view: (f32, f32)) -> (f64, f64, f64, f64) {
    for _ in 0..4 {
        host.frame(view, 1.0, FrameHooks::default());
    }
    let tree = host.accessibility_tree();
    let bounds = tree
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (Role::from_accesskit(node.role()) == Role::TextInput)
                .then(|| node.bounds())
                .flatten()
        })
        .expect("visible message editor");
    (
        bounds.x0,
        bounds.y0,
        bounds.x1 - bounds.x0,
        bounds.y1 - bounds.y0,
    )
}
