use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sigil::app::cell_width;
use sigil::prelude::ImageSource;
use sigil::prelude::Signal;
use sigil::prelude::signal;
use sigil::reactive::batch;
use sigil::widgets::TextEditState;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use crate::command::argument;
use crate::endpoint::State;
use crate::prefs::{self, Hotkey, Prefs};
use crate::provider::{Image, UserInput};
use crate::recent::Seat;
use crate::tools::IMAGES_MAX_BYTES;
use crate::workflow::Role;

pub(super) use super::chat;

use super::canvas::cell;
pub(super) use super::model;
use super::model::{Entry, Model, preview};
use super::pane::{Layout, Pane};
pub(super) use super::scene;
pub(super) use super::settings;
pub(super) use super::suggest;
use super::suggest::Picker;
use super::theme;
pub(super) use crate::tui::{Activity, Cmd, Info, Msg};

mod adjust;
mod attachments;
mod auth;
pub(super) use auth::{Login, matching, section};
mod commands;
mod composer;
mod diff;
mod elicit;
pub(super) use elicit::{FormState, Reply, Stage};
mod files;
mod frame;
mod history;
mod jobs;
mod mcp;
mod models;
mod overlays;
mod panes;
mod peers;
pub(super) mod presentation;
mod sessions;
mod turn;
mod update;
mod viewer;
mod workflow;

pub(super) use files::{Action, Context, FileRow};
use files::{REPEAT_WINDOW, armed, file_url, read_git_branch, save};

mod secret;
mod state;

pub(super) use state::{AuthDialog, HotkeyDialog, ModelPicker, WorkflowDialog};

use presentation::{facts, header, settings_text, status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Overlay {
    Secret,
    Help,
    Search,
    Sessions,
    Auth,
    Model,
    Effort,
    Workflow,
    Hotkeys,
    Settings,
    Context,
    Cache,
    Usage,
}

#[derive(Clone)]
pub(super) struct App {
    pub(super) inline: bool,
    pub(super) model: Signal<Rc<Model>>,
    pub(super) search: super::search::Picker,
    pub(super) help: super::help::View,
    pub(super) cmd: UnboundedSender<Cmd>,
    pub(super) info: Info,
    pub(super) workspace: PathBuf,
    pub(super) roots: Vec<String>,
    pub(super) git_branch: Option<String>,
    pub(super) prefs_path: Option<PathBuf>,
    pub(super) max_scroll: Signal<f32>,
    pub(super) content_height: Signal<f32>,
    pub(super) scroll_y: Signal<f32>,
    pub(super) draft: Signal<String>,
    pub(super) caret: Signal<usize>,
    pub(super) editor: TextEditState,
    pub(super) editor_focus: Signal<u64>,
    recalled: Signal<Option<(usize, String, usize)>>,
    cleared: Signal<Option<String>>,
    pub(super) header: Signal<String>,
    pub(super) status: Signal<String>,
    pub(super) settings: Signal<String>,
    pub(super) facts: Signal<String>,
    pub(super) settings_open: Signal<bool>,
    pub(super) context_open: Signal<bool>,
    pub(super) cache_open: Signal<bool>,
    pub(super) meter_hover: Signal<Option<super::chat::Target>>,
    pub(super) scroll: Signal<usize>,
    pub(super) settings_sel: Signal<usize>,
    pub(super) modules: Signal<std::collections::BTreeMap<crate::modules::ModuleId, bool>>,
    pub(super) ticks: Signal<u64>,
    pub(super) splash_revealed: Signal<bool>,
    pub(super) splash_since: Rc<Cell<std::time::Instant>>,
    pub(super) splash: Signal<bool>,
    pub(super) theme_name: Signal<String>,
    pub(super) viewport: Signal<(u16, u16)>,
    pub(super) layout: Signal<Layout>,
    pub(super) pane_focus: Signal<bool>,
    pub(super) file_cursor: Signal<usize>,
    pub(super) expanded: Signal<Vec<String>>,
    pub(super) last_pane: Signal<Option<Pane>>,
    pub(super) context: Signal<Option<Context>>,
    pub(super) diff_cursor: Signal<usize>,
    pub(super) diff_view: Signal<Option<String>>,
    pub(super) diff_root: Signal<Option<(PathBuf, String)>>,
    pub(super) hunk_cursor: Signal<usize>,
    pub(super) note_target: Signal<Option<(String, String)>>,
    pub(super) note_text: Signal<String>,
    pub(super) note_caret: Signal<usize>,
    pub(super) tree_cursor: Signal<usize>,
    pub(super) job_cursor: Signal<usize>,
    pub(super) job_open: Signal<Option<String>>,
    pub(super) mcp_cursor: Signal<usize>,
    pub(super) mcp_open: Signal<Option<String>>,
    pub(super) peer_cursor: Signal<usize>,
    pub(super) view_scroll: Signal<usize>,
    pub(super) view_from: Signal<Option<Pane>>,
    pub(super) diff_recent: Signal<bool>,
    pub(super) view_edit: Signal<bool>,
    pub(super) view_text: Signal<String>,
    pub(super) view_caret: Signal<usize>,
    view_saved: Signal<String>,
    discard_armed: Signal<bool>,
    pub(super) copy: Signal<Option<String>>,
    pub(super) approval: Signal<Option<(String, bool)>>,
    pub(super) form: Signal<Option<FormState>>,
    pub(super) secret: Signal<Option<(String, usize)>>,
    pub(super) secret_dialog: Signal<bool>,
    pending: Rc<RefCell<Option<(crate::askpass::Request, crate::askpass::Secret)>>>,
    pub(super) select: Signal<bool>,
    copy_pending: Signal<Option<(Option<PathBuf>, String)>>,
    pub(super) open_file: Signal<Option<PathBuf>>,
    pub(super) quit: Signal<bool>,
    quit_armed: Rc<Cell<Option<std::time::Instant>>>,
    escape_armed: Rc<Cell<Option<std::time::Instant>>>,
    delete_armed: Rc<Cell<Option<std::time::Instant>>>,
    pub(super) editor_requested: Signal<bool>,
    pub(super) commands: Picker,
    pub(super) sessions: super::sessions::Picker,
    pub(super) usage: super::usage::Panel,
    pub(super) effort: super::effort::Slider,
    pub(super) picker: ModelPicker,
    pub(super) workflow: WorkflowDialog,
    pub(super) auth: AuthDialog,
    pub(super) hotkeys: HotkeyDialog,
    attachments: Rc<RefCell<Vec<Attachment>>>,
    pub(super) attached: Signal<usize>,
    pasted_text: Rc<RefCell<Vec<String>>>,
}

pub(super) struct Attachment {
    pub(super) image: crate::provider::Image,
    pub(super) source: Option<ImageSource>,
    pub(super) label: String,
}

impl Attachment {
    fn new(image: crate::provider::Image, source: ImageSource, label: String) -> Self {
        Self {
            image,
            source: Some(source),
            label,
        }
    }

    fn restore(image: crate::provider::Image) -> Self {
        let source = image.bytes().ok().and_then(|b| preview(&b).ok());
        let label = format!("image · {}", image.media_type);
        Self {
            image,
            source,
            label,
        }
    }
}

impl App {
    pub(super) fn new(info: Info, workspace: PathBuf, cmd: UnboundedSender<Cmd>) -> Self {
        theme::install(&info.prefs, &workspace, info.config.as_deref());
        let model = Model::new(&info);
        let git_branch = read_git_branch(&workspace);
        let draft = signal(String::new());
        let caret = signal(0);
        let app = Self {
            inline: info.inline,
            model: signal(Rc::new(model)),
            search: super::search::Picker::new(),
            help: super::help::View::new(),
            cmd,
            max_scroll: signal(0.0),
            content_height: signal(0.0),
            scroll_y: signal(0.0),
            draft,
            caret,
            editor: TextEditState::new(draft, caret),
            editor_focus: signal(0),
            recalled: signal(None),
            cleared: signal(None),
            header: signal(String::new()),
            status: signal(String::new()),
            settings: signal(String::new()),
            facts: signal(String::new()),
            settings_open: signal(false),
            context_open: signal(false),
            cache_open: signal(false),
            meter_hover: signal(None),
            scroll: signal(0),
            settings_sel: signal(0),
            modules: signal({
                let switches = crate::modules::Switches::load(&workspace).ok();
                crate::modules::ALL
                    .iter()
                    .map(|module| {
                        let on = switches
                            .as_ref()
                            .is_none_or(|switches| switches.enabled(module.id));
                        (module.id, on)
                    })
                    .collect()
            }),
            ticks: signal(0),
            splash_revealed: signal(!info.prefs.animations),
            splash_since: Rc::new(Cell::new(std::time::Instant::now())),
            splash: signal(info.splash),
            theme_name: signal(info.prefs.theme.clone()),
            viewport: signal((80, 24)),
            layout: signal(Layout::parse(&info.prefs.layout).unwrap_or_default()),
            pane_focus: signal(false),
            file_cursor: signal(0),
            expanded: signal(Vec::new()),
            last_pane: signal(None),
            context: signal(None),
            diff_cursor: signal(0),
            diff_view: signal(None),
            diff_root: signal(None),
            hunk_cursor: signal(0),
            note_target: signal(None),
            note_text: signal(String::new()),
            note_caret: signal(0),
            tree_cursor: signal(0),
            job_cursor: signal(0),
            job_open: signal(None),
            mcp_cursor: signal(0),
            mcp_open: signal(None),
            peer_cursor: signal(0),
            view_scroll: signal(0),
            view_from: signal(None),
            diff_recent: signal(false),
            view_edit: signal(false),
            view_text: signal(String::new()),
            view_caret: signal(0),
            view_saved: signal(String::new()),
            discard_armed: signal(false),
            copy: signal(None),
            approval: signal(None),
            form: signal(None),
            secret: signal(None),
            secret_dialog: signal(false),
            pending: Rc::new(RefCell::new(None)),
            select: signal(false),
            copy_pending: signal(None),
            open_file: signal(None),
            quit: signal(false),
            quit_armed: Rc::new(Cell::new(None)),
            escape_armed: Rc::new(Cell::new(None)),
            delete_armed: Rc::new(Cell::new(None)),
            editor_requested: signal(false),
            commands: Picker::new(),
            sessions: super::sessions::Picker::new(),
            usage: super::usage::Panel::new(),
            effort: super::effort::Slider::new(),
            picker: ModelPicker::new(),
            workflow: WorkflowDialog::new(),
            auth: AuthDialog::new(),
            hotkeys: HotkeyDialog::new(),
            attachments: Rc::new(RefCell::new(Vec::new())),
            attached: signal(0),
            pasted_text: Rc::new(RefCell::new(Vec::new())),
            git_branch,
            roots: roots(&workspace),
            workspace,
            prefs_path: info.config.clone(),
            info,
        };
        app.cancel_secret_on_close();
        drop(app.cmd.send(Cmd::Offers));
        app
    }

    pub(super) fn model_mut(&self, change: impl FnOnce(&mut Model)) {
        self.model.update(|model| change(Rc::make_mut(model)));
    }

    pub(super) fn push_entry(&self, entry: Entry) {
        self.model_mut(|model| model.entries.push(entry));
    }
}

fn roots(workspace: &std::path::Path) -> Vec<String> {
    let real = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let mut roots = vec![real.display().to_string(), workspace.display().to_string()];
    roots.dedup();
    roots.retain(|root| root.len() > 1);
    roots
}
