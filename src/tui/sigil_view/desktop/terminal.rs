use std::path::{Path, PathBuf};
use std::rc::Rc;

use sigil::app::Session;
use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, ControlSize, LayoutStyle, ListRowSlots, NodeBuilder, Paint, Signal, Tone, Widget, button, container,
    effect, list_row, signal,
};
use sigil::reactive::{Owner, untrack};
use sigil::widgets::{MenuItem, icon};

use super::App;
use super::style::{DIVIDER, ICON, S, Text, text};

const TITLE_CHARS: usize = 60;
const SHELL: &str = "Shell";
const EXITED: &str = "exited";
const BELL: &str = "Bell";

/// One running terminal, shared by every pane in the window.
pub(in crate::tui::sigil_view) struct Term {
    pub(super) id: u64,
    pub(super) folder: PathBuf,
    pub(super) session: Rc<Session>,
    name: &'static str,
    seen: Signal<u64>,
}

impl Term {
    /// What the terminal is called: the program's title, else its name; tracked.
    pub(super) fn label(&self) -> String {
        let title = self.session.title.get();
        let title = title.trim();
        if title.is_empty() { self.name.to_owned() } else { title.chars().take(TITLE_CHARS).collect() }
    }
}

/// The window's terminals and the owner they outlive their panes under.
#[derive(Clone, Copy)]
pub(in crate::tui::sigil_view) struct Terminals {
    pub(super) list: Signal<Vec<Rc<Term>>>,
    next: Signal<u64>,
    owner: Option<Owner>,
}

impl Terminals {
    /// Starts an empty list held by the current owner.
    pub(in crate::tui::sigil_view) fn new() -> Self {
        Self { list: signal(Vec::new()), next: signal(1), owner: Owner::current() }
    }

    fn start(self, folder: &Path, name: &'static str) -> Result<Rc<Term>, String> {
        let owner = self.owner.ok_or_else(|| "the window has no owner".to_owned())?;
        let (session, seen) = owner.with(|| {
            let session = Session::shell_in(folder).map_err(|error| error.to_string())?;
            Ok::<_, String>((session, signal(0)))
        })?;
        let id = self.next.peek();
        self.next.set(id + 1);
        let term = Rc::new(Term { id, folder: folder.to_path_buf(), session: Rc::new(session), name, seen });
        self.list.update(|list| list.push(Rc::clone(&term)));
        Ok(term)
    }
}

impl App {
    /// The terminal this pane shows instead of its agent; tracked.
    pub(in crate::tui::sigil_view) fn shown_terminal(&self) -> Option<Rc<Term>> {
        let id = self.showing.get()?;
        self.terminals.list.with(|list| list.iter().find(|term| term.id == id).cloned())
    }

    /// Opens a shell in the focused pane's folder and shows it there, running `command` when given.
    pub(in crate::tui::sigil_view) fn open_terminal(&self, name: &'static str, command: Option<&str>) {
        let pane = super::split::current(self);
        match self.terminals.start(&pane.workspace(), name) {
            Ok(term) => {
                if let Some(command) = command {
                    drop(term.session.write(format!("{command}\r").as_bytes()));
                }
                pane.showing.set(Some(term.id));
            }
            Err(error) => pane.push_entry(crate::tui::sigil_view::model::Entry::Error(format!(
                "Could not start a terminal: {error}"
            ))),
        }
    }
}

/// The name of the program a plain new terminal runs.
pub(in crate::tui::sigil_view) const NEW_TERMINAL: &str = "Terminal";
/// The sidebar action that opens a terminal.
pub(super) const NEW_TERMINAL_ACTION: &str = "New terminal";
/// The name of a terminal that starts Claude Code.
pub(super) const CLAUDE: &str = "Claude Code";
/// The command that starts Claude Code.
pub(super) const CLAUDE_COMMAND: &str = "claude";

/// Shows terminal `id`, focusing the pane that already shows it.
pub(super) fn reveal(app: &App, id: u64) {
    if !super::split::focus_terminal(id) {
        super::split::current(app).showing.set(Some(id));
    }
}

/// Ends terminal `id` and removes it.
pub(super) fn close(app: &App, id: u64) {
    for pane in super::split::apps() {
        if pane.showing.peek() == Some(id) {
            pane.showing.set(None);
        }
    }
    app.terminals.list.update(|list| list.retain(|term| term.id != id));
}

/// A pane's body: its terminal when it shows one, else its agent.
pub(super) fn pane(app: App) -> NodeBuilder {
    let showing = app.showing;
    container()
        .layout(LayoutStyle::flex_column().grow(1.0).fill_width().fill_height().min_size(0.0, 0.0))
        .children_reactive(move || {
            let id = showing.get();
            let term = id.and_then(|id| app.terminals.list.peek().iter().find(|term| term.id == id).cloned());
            vec![untrack(|| term.map_or_else(|| super::thread::main_col(app.clone()), |term| main(&app, &term)))]
        })
}

fn main(app: &App, term: &Rc<Term>) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_column().grow(1.0).fill_width().fill_height().min_size(0.0, 0.0))
        .bg(super::style::CANVAS)
        .child(super::thread::header(app))
        .child(body(app, term))
}

fn body(app: &App, term: &Rc<Term>) -> NodeBuilder {
    let (bell, seen) = (term.session.bell, term.seen);
    effect(move || seen.set(bell.get()));
    let (watch, closing, id) = (term.session.exited, app.clone(), term.id);
    container()
        .layout(LayoutStyle::flex_column().grow(1.0).fill_width().min_size(0.0, 0.0).padding(S))
        .child(
            container()
                .layout(LayoutStyle::flex_column().grow(1.0).fill_width().min_size(0.0, 0.0))
                .child(term.session.view().label(SHELL).autofocus()),
        )
        .children_reactive(move || watch.get().map(|code| ended(&closing, id, code)).into_iter().collect::<Vec<NodeBuilder>>())
}

fn ended(app: &App, id: u64, code: u32) -> NodeBuilder {
    let app = app.clone();
    container()
        .layout(LayoutStyle::flex_row().fill_width().gap(S).padding_top(S).shrink(0.0).align_items(AlignItems::Center))
        .border_top(1)
        .border_color(DIVIDER)
        .child(text(Text::Hint, format!("The program exited with code {code}.")))
        .child(button("Close").ghost().size(ControlSize::Compact).activate(move || close(&app, id)))
}

/// The right panel's terminal: the folder's first one no pane shows, started the first time.
pub(super) fn view(app: &App, width: f32) -> NodeBuilder {
    let folder = app.workspace();
    let column = container()
        .layout(LayoutStyle::flex_column().width(width).fill_height().padding(S).min_size(0.0, 0.0));
    match side_terminal(app, &folder) {
        Ok(term) => column.child(term.session.view().label(SHELL).autofocus()),
        Err(error) => column.child(text(Text::Hint, format!("Could not start a shell: {error}"))),
    }
}

fn side_terminal(app: &App, folder: &Path) -> Result<Rc<Term>, String> {
    let shown: Vec<u64> = super::split::apps().iter().filter_map(|pane| pane.showing.peek()).collect();
    let list = app.terminals.list.peek();
    let free = list
        .iter()
        .find(|term| term.folder == folder && !shown.contains(&term.id) && term.session.exited.peek().is_none())
        .cloned();
    free.map_or_else(|| app.terminals.start(folder, NEW_TERMINAL), Ok)
}

/// The sidebar rows of the terminals in `folder`.
pub(super) fn rows(app: &App, terms: &[Rc<Term>], folder: &Path) -> Vec<NodeBuilder> {
    terms.iter().filter(|term| term.folder == folder).map(|term| row(app, term)).collect()
}

fn row(app: &App, term: &Rc<Term>) -> NodeBuilder {
    let (app, term) = (app.clone(), Rc::clone(term));
    container().layout(LayoutStyle::block().fill_width()).children_reactive(move || {
        let selected = super::split::current(&app).showing.get() == Some(term.id);
        let rang = term.session.bell.get() != term.seen.get();
        let done = term.session.exited.get().is_some();
        let (open, id, menu) = (app.clone(), term.id, app.clone());
        let row = list_row(
            selected,
            term.label(),
            ListRowSlots {
                leading: Some(Box::new(icon(Icon::Terminal).px(ICON))),
                status: done.then(|| EXITED.into()),
                badge: rang.then(|| (BELL.into(), Tone::Info)),
                activate: Some(Rc::new(move || reveal(&open, id))),
                ..ListRowSlots::default()
            },
        );
        vec![sigil::widgets::context_menu(row, move || {
            let closing = menu.clone();
            vec![MenuItem::new("Close terminal").activate(move || close(&closing, id))]
        })]
    })
}
