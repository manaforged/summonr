use super::*;

#[derive(Clone)]
pub(in crate::tui::sigil_view) struct ModelPicker {
    pub(in crate::tui::sigil_view) open: Signal<bool>,
    pub(in crate::tui::sigil_view) draft: Signal<String>,
    pub(in crate::tui::sigil_view) choices: Signal<Rc<Vec<Seat>>>,
    pub(in crate::tui::sigil_view) shown: Signal<Rc<Vec<Seat>>>,
    pub(in crate::tui::sigil_view) facts:
        Signal<Rc<std::collections::BTreeMap<Seat, crate::catalog::Model>>>,
    pub(in crate::tui::sigil_view) default: Signal<Option<Seat>>,
    pub(in crate::tui::sigil_view) groups: Signal<std::collections::BTreeMap<String, State>>,
    pub(in crate::tui::sigil_view) causes: Signal<std::collections::BTreeMap<String, String>>,
    pub(in crate::tui::sigil_view) recent: Signal<[usize; 3]>,
    pub(in crate::tui::sigil_view) older: Signal<bool>,
    pub(in crate::tui::sigil_view) facet: Signal<super::facet::Facet>,
    pub(in crate::tui::sigil_view) hidden: Signal<usize>,
    pub(in crate::tui::sigil_view) note: Signal<String>,
    pub(in crate::tui::sigil_view) sel: Signal<usize>,
    pub(in crate::tui::sigil_view) loading: Signal<bool>,
    pub(in crate::tui::sigil_view) error: Signal<String>,
    pub(in crate::tui::sigil_view) request: Rc<Cell<u64>>,
    pub(in crate::tui::sigil_view) cancel: Rc<RefCell<Option<oneshot::Sender<()>>>>,
    pub(in crate::tui::sigil_view) target: Signal<Option<usize>>,
    pub(in crate::tui::sigil_view) focus: Signal<u64>,
    pub(in crate::tui::sigil_view) scroll: Signal<f32>,
}

impl ModelPicker {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            draft: signal(String::new()),
            choices: signal(Rc::default()),
            shown: signal(Rc::default()),
            facts: signal(Rc::default()),
            default: signal(None),
            groups: signal(std::collections::BTreeMap::new()),
            causes: signal(std::collections::BTreeMap::new()),
            recent: signal([0; 3]),
            older: signal(false),
            facet: signal(super::facet::Facet::All),
            hidden: signal(0),
            note: signal(String::new()),
            sel: signal(0),
            loading: signal(false),
            error: signal(String::new()),
            request: Rc::new(Cell::new(0)),
            cancel: Rc::new(RefCell::new(None)),
            target: signal(None),
            focus: signal(0),
            scroll: signal(0.0),
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::tui::sigil_view) struct WorkflowDialog {
    pub(in crate::tui::sigil_view) open: Signal<bool>,
    pub(in crate::tui::sigil_view) name: Signal<String>,
    pub(in crate::tui::sigil_view) names: Signal<Vec<String>>,
    pub(in crate::tui::sigil_view) seats: Signal<Vec<Option<String>>>,
    pub(in crate::tui::sigil_view) sel: Signal<usize>,
    pub(in crate::tui::sigil_view) naming: Signal<bool>,
    pub(in crate::tui::sigil_view) draft: Signal<String>,
    pub(in crate::tui::sigil_view) caret: Signal<usize>,
    pub(in crate::tui::sigil_view) drag_from: Signal<Option<usize>>,
    pub(in crate::tui::sigil_view) drag_over: Signal<Option<usize>>,
}

impl WorkflowDialog {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            name: signal(String::new()),
            names: signal(Vec::new()),
            seats: signal(vec![None; Role::ALL.len()]),
            sel: signal(0),
            naming: signal(false),
            draft: signal(String::new()),
            caret: signal(0),
            drag_from: signal(None),
            drag_over: signal(None),
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::tui::sigil_view) struct AuthDialog {
    pub(in crate::tui::sigil_view) open: Signal<bool>,
    pub(in crate::tui::sigil_view) choices: Signal<Vec<crate::endpoint::AuthInfo>>,
    pub(in crate::tui::sigil_view) sel: Signal<usize>,
    pub(in crate::tui::sigil_view) step: Signal<Login>,
    pub(in crate::tui::sigil_view) filter: Signal<String>,
    pub(in crate::tui::sigil_view) seat: Signal<Option<(String, String)>>,
    pub(in crate::tui::sigil_view) focus: Signal<u64>,
    pub(in crate::tui::sigil_view) key: Signal<String>,
    pub(in crate::tui::sigil_view) caret: Signal<usize>,
    pub(in crate::tui::sigil_view) scroll: Signal<f32>,
}

impl AuthDialog {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            choices: signal(Vec::new()),
            sel: signal(0),
            step: signal(Login::List(super::auth::Note::default())),
            filter: signal(String::new()),
            seat: signal(None),
            focus: signal(0),
            key: signal(String::new()),
            caret: signal(0),
            scroll: signal(0.0),
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::tui::sigil_view) struct HotkeyDialog {
    pub(in crate::tui::sigil_view) open: Signal<bool>,
    pub(in crate::tui::sigil_view) sel: Signal<usize>,
    pub(in crate::tui::sigil_view) recording: Signal<bool>,
    pub(in crate::tui::sigil_view) armed: Signal<bool>,
    pub(in crate::tui::sigil_view) note: Signal<String>,
}

impl HotkeyDialog {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            sel: signal(0),
            recording: signal(false),
            armed: signal(false),
            note: signal(String::new()),
        }
    }
}
