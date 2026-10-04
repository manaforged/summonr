use sigil::prelude::{KeyEvent, LogicalKey, ModifiersState, NodeBuilder, NodeEventsExt, Shortcut};

use crate::tui::sigil_view::app::App;

const OPTION_GLYPHS: [(&str, &str); 3] = [("p", "π"), ("d", "∂"), ("o", "ø")];

pub(in crate::tui::sigil_view) type KeyAction = fn(&App, &mut KeyEvent);
pub(in crate::tui::sigil_view) type Bindings = Vec<(Shortcut, KeyAction)>;

pub(in crate::tui::sigil_view) fn press(
    key: &'static str,
    action: KeyAction,
) -> (Shortcut, KeyAction) {
    (LogicalKey::character(key).into(), action)
}

pub(in crate::tui::sigil_view) fn alt(letter: &'static str, action: KeyAction) -> Bindings {
    let mut bindings = vec![(
        Shortcut::new(LogicalKey::character(letter), ModifiersState::ALT),
        action,
    )];
    bindings.extend(
        OPTION_GLYPHS
            .iter()
            .filter(|(key, _)| *key == letter)
            .map(|(_, glyph)| press(glyph, action)),
    );
    bindings
}

pub(in crate::tui::sigil_view) fn keyed(
    node: NodeBuilder,
    app: &App,
    bindings: Bindings,
) -> NodeBuilder {
    bindings.into_iter().fold(node, |node, (shortcut, action)| {
        let app = app.clone();
        node.key_capture(shortcut, move |event| action(&app, event))
    })
}
