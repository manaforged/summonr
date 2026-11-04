use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use sigil::prelude::{LayoutStyle, NodeBuilder, Widget, container, effect, signal};
use sigil::reactive::defer;
use sigil::reactive::untrack;
use sigil::tasks::on_message;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use super::app::App;
use super::desktop::{root, welcome};
use super::host::{start_tick, stop};
use super::model::Entry;
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tui::hub::{Factory, Started, start};
use crate::tui::{Cmd, Info, Intent, Msg, Ui};

use summonr_core::error::{Error, Result};

const TITLE: &str = env!("CARGO_PKG_NAME");
const SIZE: (u32, u32) = (1280, 800);

struct Prepared {
    loop_: Loop<Client>,
    info: Info,
    factory: Factory,
    ui: Ui,
    inbox: crate::tui::Inbox,
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
    let prepare = Arc::new(prepare);
    let running = Rc::new(RefCell::new(None::<Running>));
    let worker = running.clone();
    let opened = tokio::task::block_in_place(|| {
        sigil::run(window(), move || {
            application(prepare.clone(), running.clone())
        })
    });
    if let Some(running) = worker.borrow_mut().take() {
        running.signals.abort();
        stop(
            running.worker,
            &running.commands,
            crate::limits::limits().worker_stop(),
        );
    }
    opened.map_err(|error| Error::Tool(format!("the window failed: {error}")))?;
    Ok(0)
}

fn application(prepare: Arc<Prepare>, running: Rc<RefCell<Option<Running>>>) -> NodeBuilder {
    super::canvas::window_cells();
    let ready = signal(false);
    let busy = signal(false);
    let error = signal(None::<String>);
    let prepared = Rc::new(RefCell::new(None));
    let received = prepared.clone();
    let (out, inbox) = unbounded_channel::<Result<Prepared>>();
    on_message(
        inbox,
        move |result| {
            busy.set(false);
            match result {
                Ok(agent) => {
                    *received.borrow_mut() = Some(agent);
                    ready.set(true);
                }
                Err(cause) => error.set(Some(cause.to_string())),
            }
        },
        || {},
    );
    container()
        .layout(LayoutStyle::flex_column().fill())
        .children_reactive(move || {
            let _ready = ready.get();
            if let Some(prepared) = prepared.borrow_mut().take() {
                return vec![untrack(|| {
                    let (app, worker) = mount(prepared);
                    *running.borrow_mut() = Some(worker);
                    root(app)
                })];
            }
            let (prepare, out) = (prepare.clone(), out.clone());
            vec![welcome::view(error, busy, move |workspace| {
                if busy.peek() {
                    return;
                }
                busy.set(true);
                error.set(None);
                let (prepare, out) = (prepare.clone(), out.clone());
                tokio::task::spawn_blocking(move || {
                    drop(out.send(Prepared::new(&*prepare, workspace)));
                });
            })]
        })
}

impl Prepared {
    fn new(prepare: &Prepare, workspace: PathBuf) -> Result<Self> {
        let (ui, inbox) = crate::tui::channel();
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
        crate::interrupted().await;
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
            let key = model.prefs.hotkeys.label(crate::prefs::Hotkey::Editor);
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
