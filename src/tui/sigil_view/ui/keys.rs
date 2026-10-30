use std::cell::Cell;
use std::rc::Rc;

use sigil::kernel::TextCursor;
use sigil::prelude::{
    KeyEvent, LogicalKey, ModifiersState, NodeBuilder, NodeEventsExt, Shortcut, WidgetLayoutExt,
};

use crate::prefs::EditorKey;
use crate::tui::sigil_view::app::App;

const OPTION_GLYPHS: [(&str, &str); 3] = [("p", "π"), ("d", "∂"), ("o", "ø")];

pub(in crate::tui::sigil_view) type KeyAction = fn(&App, &mut KeyEvent);
pub(in crate::tui::sigil_view) type Bindings = Vec<(Shortcut, KeyAction)>;

pub(in crate::tui::sigil_view) fn input_keys<T: WidgetLayoutExt>(app: &App, input: T) -> T {
    let app = app.clone();
    input.map_node(move |node| {
        let cursor = Rc::new(Cell::new(None::<TextCursor>));
        let field = Rc::clone(&cursor);
        node.mutate(move |node| field.set(node.a11y.text_cursor))
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
