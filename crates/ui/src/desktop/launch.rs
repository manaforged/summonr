use std::cell::{OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use sigil::prelude::{LayoutStyle, NodeBuilder, Signal, Widget, container, effect, signal};
use sigil::reactive::untrack;
use sigil::reactive::{Owner, defer};
use sigil::tasks::on_message;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use crate::app::App;
use crate::desktop::{root, welcome};
use crate::host::run::{start_tick, stop};
use crate::model::Entry;
use summonr_agent::hub::{Cmd, Info, Intent, Msg, Ui};
use summonr_agent::hub::{Factory, Started, start};
use summonr_agent::provider::Client;
use summonr_agent::runtime::Loop;

use summonr_core::error::{Error, Result};

const TITLE: &str = crate::PRODUCT;
const SIZE: (u32, u32) = (1280, 800);

struct Prepared {
    loop_: Loop<Client>,
    info: Info,
    factory: Factory,
    ui: Ui,
    inbox: summonr_agent::hub::Inbox,
}

type Prepare = dyn Fn(PathBuf, Ui) -> Result<(Loop<Client>, Info, Factory)> + Send + Sync;

struct Running {
    commands: UnboundedSender<Cmd>,
    worker: JoinHandle<()>,
    signals: JoinHandle<()>,
}

/// Launch the native application and prepare its agent after the user selects a project.
pub fn run(
    prepare: impl Fn(PathBuf, Ui) -> Result<(Loop<Client>, Info, Factory)> + Send + Sync + 'static,
) -> Result<i32> {
    let prepare: Arc<Prepare> = Arc::new(prepare);
    let running = Rc::new(RefCell::new(None::<Running>));
    let worker = running.clone();
    let session = Rc::new(OnceCell::new());
    let opened = tokio::task::block_in_place(|| {
        sigil::run(window(), move || application(&prepare, &running, &session))
    });
    if let Some(running) = worker.borrow_mut().take() {
        running.signals.abort();
        stop(
            running.worker,
            &running.commands,
            summonr_agent::limits::limits().worker_stop(),
        );
    }
    opened.map_err(|error| Error::Tool(format!("the window failed: {error}")))?;
    Ok(0)
}

fn application(
    prepare: &Arc<Prepare>,
    running: &Rc<RefCell<Option<Running>>>,
    session: &Rc<OnceCell<Session>>,
) -> NodeBuilder {
    crate::desktop::host::window_cells();
    let session = session
        .get_or_init(|| {
            let owner = Owner::new("summonr::desktop", None);
            owner.with(|| Session::start(prepare, owner))
        })
        .clone();
    let (prepare, running) = (prepare.clone(), running.clone());
    container()
        .layout(LayoutStyle::flex_column().fill())
        .children_reactive(move || {
            let _ready = session.ready.get();
            if let Some(prepared) = session.prepared.borrow_mut().take() {
                let (app, worker) = session.owner.with(|| untrack(|| mount(prepared)));
                crate::desktop::probe_local(&app);
                *running.borrow_mut() = Some(worker);
                *session.app.borrow_mut() = Some(app);
            }
            if let Some(app) = session.app.borrow().clone() {
                return vec![untrack(|| root(app))];
            }
            if session.launching.get() {
                return Vec::new();
            }
            vec![session.welcome(&prepare)]
        })
}

#[derive(Clone)]
struct Session {
    owner: Owner,
    ready: Signal<bool>,
    busy: Signal<bool>,
    error: Signal<Option<String>>,
    launching: Signal<bool>,
    prepared: Rc<RefCell<Option<Prepared>>>,
    app: Rc<RefCell<Option<App>>>,
    out: UnboundedSender<Result<Prepared>>,
}

impl Session {
    fn start(prepare: &Arc<Prepare>, owner: Owner) -> Self {
        let (out, inbox) = unbounded_channel::<Result<Prepared>>();
        let session = Self {
            owner,
            ready: signal(false),
            busy: signal(false),
            error: signal(None),
            launching: signal(false),
            prepared: Rc::new(RefCell::new(None)),
            app: Rc::new(RefCell::new(None)),
            out,
        };
        let received = session.clone();
        on_message(inbox, move |result| received.received(result), || {});
        if let Some(folder) = welcome::last() {
            session.launching.set(true);
            session.open(prepare, folder);
        }
        session
    }

    fn received(&self, result: Result<Prepared>) {
        self.busy.set(false);
        match result {
            Ok(agent) => {
                *self.prepared.borrow_mut() = Some(agent);
                self.ready.set(true);
            }
            Err(cause) => {
                self.launching.set(false);
                self.error.set(Some(cause.to_string()));
            }
        }
    }

    fn open(&self, prepare: &Arc<Prepare>, workspace: PathBuf) {
        self.busy.set(true);
        let (prepare, out) = (prepare.clone(), self.out.clone());
        tokio::task::spawn_blocking(move || drop(out.send(Prepared::new(&*prepare, workspace))));
    }

    fn welcome(&self, prepare: &Arc<Prepare>) -> NodeBuilder {
        let (session, prepare) = (self.clone(), prepare.clone());
        welcome::view(self.error, self.busy, move |workspace| {
            if session.busy.peek() {
                return;
            }
            session.error.set(None);
            session.open(&prepare, workspace);
        })
    }
}

impl Prepared {
    fn new(prepare: &Prepare, workspace: PathBuf) -> Result<Self> {
        let (ui, inbox) = summonr_agent::hub::channel();
        let (loop_, info, factory) = prepare(workspace, ui.clone())?;
        Ok(Self {
            loop_,
            info,
            factory,
            ui,
            inbox,
        })
    }
}

fn mount(prepared: Prepared) -> (App, Running) {
    let Prepared {
        loop_,
        info,
        factory,
        ui,
        inbox,
    } = prepared;
    let workspace = loop_.workspace().to_path_buf();
    let farewell = ui.clone();
    let signals = tokio::spawn(async move {
        summonr_agent::interrupted().await;
        drop(farewell.0.send(Msg::Intent(Intent::Bye)));
    });
    let Started {
        commands,
        inbox,
        task: worker,
    } = start(loop_, info.clone(), (ui.0, inbox.0), factory);
    start_tick(commands.clone());
    let app = App::new(info.clone(), workspace, commands.clone());
    app.fit_window();
    app.load();
    if let Some(prompt) = &info.prompt {
        app.send_text(prompt);
    }
    app.refresh();
    app.animate();
    let receiver = app.clone();
    on_message(
        inbox,
        move |msg| {
            untrack(|| {
                receiver.update(msg);
                receiver.refresh();
            });
        },
        || {},
    );
    watch_outputs(&app);
    (
        app,
        Running {
            commands,
            worker,
            signals,
        },
    )
}

fn window() -> sigil::AppConfig {
    sigil::AppConfig::new(TITLE)
        .size(SIZE.0, SIZE.1)
        .inset_title_bar(crate::desktop::TITLE_BAR)
        .esc_exits(false)
}

fn watch_outputs(app: &App) {
    let app = app.clone();
    effect(move || {
        let pending = app.open_file.get().is_some()
            || app.copy.get().is_some()
            || app.editor_requested.get()
            || app.quit.get();
        if pending {
            let handler = app.clone();
            defer(move || outputs(&handler));
        }
    });
}

fn outputs(app: &App) {
    if let Some(path) = app.open_file.peek() {
        app.open_file.set(None);
        if let Err(error) = reveal(&path) {
            app.push_entry(Entry::Error(error.to_string()));
        }
        app.refresh();
    }
    app.copy_out();
    if app.editor_requested.peek() {
        app.editor_requested.set(false);
        app.model_mut(|model| {
            let key = model
                .prefs
                .hotkeys
                .label(summonr_agent::prefs::Hotkey::Editor);
            model.entries.push(Entry::System(format!(
                "{key} opens $EDITOR in the terminal interface · the window edits in place"
            )))
        });
        app.refresh();
    }
    if app.quit.peek() {
        sigil::request_exit();
    }
}

fn reveal(path: &Path) -> Result<()> {
    summonr_auth::open::external(path).map_err(|error| Error::Tool(error.to_string()))
}
