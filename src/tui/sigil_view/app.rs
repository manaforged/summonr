use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use sigil::clipboard;
use sigil::prelude::{ImageSource, Signal, TextEditState, signal};
use sigil::reactive::batch;
use sigil_tui::cell_width;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use crate::command::argument;
use crate::prefs::{self, Hotkey, Prefs, ToolOutput};
use crate::provider::{Image, UserInput};
use crate::tools::IMAGES_MAX_BYTES;
use crate::workflow::Role;

pub(super) use super::chat;

use super::canvas::cell;
pub(super) use super::input;
pub(super) use super::model;
use super::model::{Entry, Model, preview};
use super::pane::{Layout, Pane};
use super::patch::{self, SPLIT};
pub(super) use super::scene;
pub(super) use super::settings;
pub(super) use super::suggest;
use super::suggest::Picker;
use super::theme;
pub(super) use crate::tui::{Activity, Cmd, Info, Msg};

mod adjust;
mod attachments;
mod auth;
mod commands;
mod composer;
mod diff;
mod files;
mod frame;
mod history;
mod models;
mod overlays;
mod panes;
pub(super) mod presentation;
mod sessions;
mod turn;
mod update;
mod viewer;
mod workflow;

pub(super) use files::{Action, Context, FileRow};
use files::{REPEAT_WINDOW, armed, file_url, read_git_branch, save};

mod secret;

use presentation::{facts, header, settings_text, status};

pub(super) enum CopyStatus {
    Confirmed,
    Requested,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Overlay {
    Help,
    Search,
    Sessions,
    Auth,
    Model,
    Workflow,
    Hotkeys,
    Settings,
    Context,
    Cache,
    Usage,
}

#[derive(Clone)]
pub(super) struct App {
    pub(super) model: Rc<RefCell<Model>>,
    pub(super) search: super::search::Picker,
    pub(super) help: super::help::View,
    pub(super) cmd: UnboundedSender<Cmd>,
    pub(super) info: Info,
    pub(super) workspace: PathBuf,
    pub(super) git_branch: Option<String>,
    pub(super) prefs_path: Option<PathBuf>,
    pub(super) extent: Signal<f32>,
    pub(super) transcript_height: Signal<f32>,
    pub(super) tui_chat_active: Signal<bool>,
    pub(super) scroll_y: Signal<f32>,
    pub(super) selection_anchor: Signal<Option<(u16, u16)>>,
    pub(super) draft: Signal<String>,
    pub(super) caret: Signal<usize>,
    pub(super) editor: TextEditState,
    recalled: Signal<Option<(usize, String, usize)>>,
    pub(super) header: Signal<String>,
    pub(super) status: Signal<String>,
    pub(super) settings: Signal<String>,
    pub(super) facts: Signal<String>,
    pub(super) panes: Signal<u64>,
    pub(super) settings_open: Signal<bool>,
    pub(super) context_open: Signal<bool>,
    pub(super) cache_open: Signal<bool>,
    pub(super) meter_hover: Signal<Option<super::chat::Target>>,
    pub(super) scroll: Signal<usize>,
    pub(super) settings_sel: Signal<usize>,
    pub(super) modules: Signal<std::collections::BTreeMap<&'static str, bool>>,
    pub(super) hotkeys_open: Signal<bool>,
    pub(super) hotkey_sel: Signal<usize>,
    pub(super) hotkey_recording: Signal<bool>,
    pub(super) hotkey_note: Signal<String>,
    pub(super) editing_model: Signal<bool>,
    pub(super) model_draft: Signal<String>,
    pub(super) model_caret: Signal<usize>,
    pub(super) model_choices: Signal<Vec<String>>,
    pub(super) model_facts: Signal<std::collections::BTreeMap<String, crate::catalog::Model>>,
    pub(super) model_groups: Signal<std::collections::BTreeMap<String, String>>,
    pub(super) model_recent: Signal<usize>,
    pub(super) model_sel: Signal<usize>,
    pub(super) model_loading: Signal<bool>,
    pub(super) model_error: Signal<String>,
    model_request: Rc<Cell<u64>>,
    model_cancel: Rc<RefCell<Option<oneshot::Sender<()>>>>,
    pub(super) model_for: Signal<Option<usize>>,
    pub(super) workflow_open: Signal<bool>,
    pub(super) workflow_name: Signal<String>,
    pub(super) workflow_names: Signal<Vec<String>>,
    pub(super) workflow_seats: Signal<Vec<Option<String>>>,
    pub(super) workflow_sel: Signal<usize>,
    pub(super) workflow_naming: Signal<bool>,
    pub(super) workflow_draft: Signal<String>,
    pub(super) workflow_caret: Signal<usize>,
    pub(super) drag_from: Signal<Option<usize>>,
    pub(super) drag_over: Signal<Option<usize>>,
    pub(super) auth_open: Signal<bool>,
    pub(super) auth_choices: Signal<Vec<crate::endpoint::AuthInfo>>,
    pub(super) auth_sel: Signal<usize>,
    pub(super) auth_entering: Signal<bool>,
    pub(super) auth_focus: Signal<u64>,
    pub(super) auth_key: Signal<String>,
    pub(super) models_focus: Signal<u64>,
    pub(super) auth_caret: Signal<usize>,
    pub(super) ticks: Signal<u64>,
    pub(super) fps: Signal<u32>,
    pub(super) splash_revealed: Signal<bool>,
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
    pub(super) diff_scroll: Signal<usize>,
    pub(super) diff_root: Signal<Option<(PathBuf, String)>>,
    pub(super) tree_cursor: Signal<usize>,
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
    pub(super) secret: Signal<Option<(String, usize)>>,
    pending: Rc<RefCell<Option<(crate::askpass::Request, crate::askpass::Secret)>>>,
    pub(super) select: Signal<bool>,
    copy_pending: Signal<Option<(Option<PathBuf>, String)>>,
    pub(super) open_file: Signal<Option<PathBuf>>,
    pub(super) resume: Rc<RefCell<Option<String>>>,
    pub(super) quit: Signal<bool>,
    quit_armed: Rc<Cell<Option<std::time::Instant>>>,
    escape_armed: Rc<Cell<Option<std::time::Instant>>>,
    delete_armed: Rc<Cell<Option<std::time::Instant>>>,
    pub(super) editor_requested: Signal<bool>,
    pub(super) commands: Picker,
    pub(super) sessions: super::sessions::Picker,
    pub(super) usage: super::usage::Panel,
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
        let label = format!("📷 {}", image.media_type);
        Self {
            image,
            source,
            label,
        }
    }
}

impl App {
    pub(super) fn new(info: Info, workspace: PathBuf, cmd: UnboundedSender<Cmd>) -> Self {
        theme::install(&info.prefs.theme, &workspace, info.config.as_deref());
        let model = Model::new(&info);
        let git_branch = read_git_branch(&workspace);
        let draft = signal(String::new());
        let caret = signal(0);
        Self {
            model: Rc::new(RefCell::new(model)),
            search: super::search::Picker::new(),
            help: super::help::View::new(),
            cmd,
            extent: signal(0.0),
            transcript_height: signal(0.0),
            tui_chat_active: signal(false),
            scroll_y: signal(0.0),
            selection_anchor: signal(None),
            draft,
            caret,
            editor: TextEditState::new(draft, caret),
            recalled: signal(None),
            header: signal(String::new()),
            status: signal(String::new()),
            settings: signal(String::new()),
            facts: signal(String::new()),
            panes: signal(0),
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
                            .is_none_or(|switches| switches.enabled(module.name));
                        (module.name, on)
                    })
                    .collect()
            }),
            hotkeys_open: signal(false),
            hotkey_sel: signal(0),
            hotkey_recording: signal(false),
            hotkey_note: signal(String::new()),
            editing_model: signal(false),
            model_draft: signal(String::new()),
            model_caret: signal(0),
            model_choices: signal(Vec::new()),
            model_facts: signal(std::collections::BTreeMap::new()),
            model_groups: signal(std::collections::BTreeMap::new()),
            model_recent: signal(0),
            model_sel: signal(0),
            model_loading: signal(false),
            model_error: signal(String::new()),
            model_request: Rc::new(Cell::new(0)),
            model_cancel: Rc::new(RefCell::new(None)),
            model_for: signal(None),
            workflow_open: signal(false),
            workflow_name: signal(String::new()),
            workflow_names: signal(Vec::new()),
            workflow_seats: signal(vec![None; Role::ALL.len()]),
            workflow_sel: signal(0),
            workflow_naming: signal(false),
            workflow_draft: signal(String::new()),
            workflow_caret: signal(0),
            drag_from: signal(None),
            drag_over: signal(None),
            auth_open: signal(false),
            auth_choices: signal(Vec::new()),
            auth_sel: signal(0),
            auth_entering: signal(false),
            auth_focus: signal(0),
            auth_key: signal(String::new()),
            models_focus: signal(0),
            auth_caret: signal(0),
            ticks: signal(0),
            fps: signal(0),
            splash_revealed: signal(!info.prefs.animations),
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
            diff_scroll: signal(0),
            diff_root: signal(None),
            tree_cursor: signal(0),
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
            secret: signal(None),
            pending: Rc::new(RefCell::new(None)),
            select: signal(false),
            copy_pending: signal(None),
            open_file: signal(None),
            resume: Rc::new(RefCell::new(None)),
            quit: signal(false),
            quit_armed: Rc::new(Cell::new(None)),
            escape_armed: Rc::new(Cell::new(None)),
            delete_armed: Rc::new(Cell::new(None)),
            editor_requested: signal(false),
            commands: Picker::new(),
            sessions: super::sessions::Picker::new(),
            usage: super::usage::Panel::new(),
            attachments: Rc::new(RefCell::new(Vec::new())),
            attached: signal(0),
            pasted_text: Rc::new(RefCell::new(Vec::new())),
            git_branch,
            workspace,
            prefs_path: info.config.clone(),
            info,
        }
    }
}
