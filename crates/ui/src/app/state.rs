use super::facet::{Facet, ranked};
use super::*;
use crate::ui::List;
use sigil::prelude::derived;

#[derive(Clone)]
pub(crate) struct ModelPicker {
    pub(crate) open: Signal<bool>,
    pub(crate) list: List,
    pub(crate) choices: Signal<Rc<Vec<ModelId>>>,
    pub(crate) shown: Signal<Rc<Vec<ModelId>>>,
    pub(crate) facts:
        Signal<std::sync::Arc<std::collections::BTreeMap<ModelId, summonr_agent::catalog::Model>>>,
    pub(crate) default: Signal<Option<ModelId>>,
    pub(crate) groups: Signal<std::collections::BTreeMap<String, State>>,
    pub(crate) causes: Signal<std::collections::BTreeMap<String, String>>,
    pub(crate) recent: Signal<[usize; 3]>,
    pub(crate) older: Signal<bool>,
    pub(crate) facet: Signal<Facet>,
    pub(crate) hidden: Signal<usize>,
    pub(crate) note: Signal<String>,
    pub(crate) loading: Signal<bool>,
    pub(crate) error: Signal<String>,
    pub(crate) request: Rc<Cell<u64>>,
    pub(crate) cancel: Rc<RefCell<Option<oneshot::Sender<()>>>>,
    pub(crate) target: Signal<Option<usize>>,
    pub(crate) focus: Signal<u64>,
}

impl ModelPicker {
    pub(super) fn new() -> Self {
        let list = List::new();
        let choices = signal(Rc::<Vec<ModelId>>::default());
        let facts = signal(std::sync::Arc::<
            std::collections::BTreeMap<ModelId, summonr_agent::catalog::Model>,
        >::default());
        let groups = signal(std::collections::BTreeMap::<String, State>::new());
        let recent = signal([0; 3]);
        let older = signal(false);
        let facet = signal(Facet::All);
        let result = derived(move || {
            let (rows, hidden) = ranked(
                &choices.get(),
                recent.get(),
                &facts.get(),
                &groups.get(),
                &list.query.get(),
                facet.get(),
                older.get(),
            );
            (Rc::new(rows), hidden)
        });
        Self {
            open: signal(false),
            list,
            choices,
            shown: derived(move || result.with(|(rows, _)| Rc::clone(rows))),
            facts,
            default: signal(None),
            groups,
            causes: signal(std::collections::BTreeMap::new()),
            recent,
            older,
            facet,
            hidden: derived(move || result.with(|(_, hidden)| *hidden)),
            note: signal(String::new()),
            loading: signal(false),
            error: signal(String::new()),
            request: Rc::new(Cell::new(0)),
            cancel: Rc::new(RefCell::new(None)),
            target: signal(None),
            focus: signal(0),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct WorkflowDialog {
    pub(crate) open: Signal<bool>,
    pub(crate) list: List,
    pub(crate) name: Signal<String>,
    pub(crate) names: Signal<Vec<String>>,
    pub(crate) selections: Signal<Vec<Option<String>>>,
    pub(crate) naming: Signal<bool>,
    pub(crate) drag_from: Signal<Option<usize>>,
    pub(crate) drag_over: Signal<Option<usize>>,
}

impl WorkflowDialog {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            list: List::new(),
            name: signal(String::new()),
            names: signal(Vec::new()),
            selections: signal(vec![None; Role::ALL.len()]),
            naming: signal(false),
            drag_from: signal(None),
            drag_over: signal(None),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct AuthDialog {
    pub(crate) open: Signal<bool>,
    pub(crate) list: List,
    pub(crate) choices: Signal<Vec<summonr_agent::endpoint::AuthInfo>>,
    pub(crate) step: Signal<Login>,
    pub(crate) selection: Signal<Option<(String, String)>>,
    pub(crate) focus: Signal<u64>,
    pub(crate) key: Signal<String>,
    pub(crate) caret: Signal<usize>,
}

impl AuthDialog {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            list: List::new(),
            choices: signal(Vec::new()),
            step: signal(Login::List(super::auth::Note::default())),
            selection: signal(None),
            focus: signal(0),
            key: signal(String::new()),
            caret: signal(0),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct HotkeyDialog {
    pub(crate) open: Signal<bool>,
    pub(crate) list: List,
    pub(crate) recording: Signal<bool>,
    pub(crate) armed: Signal<bool>,
    pub(crate) note: Signal<String>,
}

impl HotkeyDialog {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            list: List::new(),
            recording: signal(false),
            armed: signal(false),
            note: signal(String::new()),
        }
    }
}
