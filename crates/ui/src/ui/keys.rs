use std::cell::Cell;
use std::rc::Rc;

use sigil::prelude::TextCursor;
use sigil::prelude::{KeyEvent, LogicalKey, ModifiersState, NodeBuilder, Shortcut, Widget};

use crate::app::App;
use summonr_agent::prefs::EditorKey;

const OPTION_GLYPHS: [(&str, &str); 3] = [("p", "π"), ("d", "∂"), ("o", "ø")];

pub(crate) type KeyAction = fn(&App, &mut KeyEvent);
pub(crate) type Bindings = Vec<(Shortcut, KeyAction)>;

pub(crate) fn input_keys<T: sigil::prelude::Widget>(app: &App, input: T) -> T {
    let app = app.clone();
    input.map_node(move |node| {
        let cursor = Rc::new(Cell::new(None::<TextCursor>));
        let field = Rc::clone(&cursor);
        node.mutate(move |node| field.set(node.text_cursor()))
            .key_capture(Shortcut::ctrl(LogicalKey::character("x")), move |event| {
                let selected = cursor.get().is_some_and(|cursor| {
                    cursor
                        .anchor
                        .peek()
                        .is_some_and(|anchor| anchor != cursor.caret.peek())
                });
                if EditorKey::yields('x', false) && selected {
                    return;
                }
                app.arm_leader();
                event.stop_propagation();
            })
    })
}

pub(crate) fn press(key: &'static str, action: KeyAction) -> (Shortcut, KeyAction) {
    (LogicalKey::character(key).into(), action)
}

pub(crate) fn alt(letter: &'static str, action: KeyAction) -> Bindings {
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

pub(crate) fn keyed(node: NodeBuilder, app: &App, bindings: Bindings) -> NodeBuilder {
    bindings.into_iter().fold(node, |node, (shortcut, action)| {
        let app = app.clone();
        node.key_capture(shortcut, move |event| action(&app, event))
    })
}
