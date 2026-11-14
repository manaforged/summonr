use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sigil::prelude::ImageSource;
use sigil::prelude::Signal;
use sigil::prelude::signal;
use sigil::reactive::batch;
use sigil::widgets::TextEditState;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use summonr_agent::command::argument;
use summonr_agent::endpoint::State;
use summonr_agent::prefs::{self, Hotkey, Prefs};
use summonr_agent::provider::{Image, UserInput};
use summonr_agent::recent::ModelId;
use summonr_agent::tools::IMAGES_MAX_BYTES;
use summonr_agent::workflow::Role;

pub(super) use super::chat;

use super::canvas::cell;
pub(super) use super::model;
use super::model::{Entry, Model, preview};
use super::pane::{Layout, Pane, Target};
pub(super) use super::settings;
pub(super) use super::suggest;
use super::suggest::Picker;
use super::theme;
pub(super) use summonr_agent::hub::{Activity, Cmd, Fact, Info, Intent, Msg};

mod adjust;
mod attachments;
pub(crate) mod auth;
pub(super) use auth::{Login, Note, matching, section};
mod commands;
mod composer;
mod diff;
mod elicit;
mod facet;
pub(super) use elicit::{FormState, Reply, Stage};
pub(super) use facet::Facet;
pub(super) use models::Mark;
mod files;
pub(crate) mod forge;
mod frame;
mod history;
mod layout;
mod models;
mod note;
pub(crate) mod overlays;
pub(crate) mod panels;
mod panes;
mod pick;
mod place;
pub(super) use pick::Pick;
pub(super) use place::Place;
pub(super) mod presentation;
mod rows;
mod sessions;
mod tabs;
mod turn;
mod update;
mod viewer;
mod workflow;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Asked {
    pub(crate) detail: String,
    pub(crate) persist: bool,
    pub(crate) dialog: u64,
}

pub(super) use super::ui::shift;
use files::{Confirm, confirm, file_url, read_git_branch, save};
pub(super) use files::{PaneRow, armed};

mod secret;
mod state;

pub(super) use state::{AuthDialog, HotkeyDialog, ModelPicker, WorkflowDialog};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Overlay {
    Secret,
    Help,
    Search,
    Sessions,
    Usage,
    Auth,
    Model,
    Effort,
    Workflow,
    Hotkeys,
    Settings,
    Context,
    Cache,
    Status,
    Theme,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoticeKind {
    Copied,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Notice {
    pub(crate) text: String,
    pub(crate) kind: NoticeKind,
}

#[derive(Clone)]
pub(super) struct App {
    pub(super) inline: bool,
    pub(super) model: Signal<Rc<Model>>,
    pub(super) search: super::search::Picker,
    pub(super) help: super::help::View,
    pub(super) cmd: UnboundedSender<Cmd>,
    pub(super) info: Info,
    pub(super) place: Signal<Place>,
    pub(super) window: crate::window::Window,
    pub(super) prefs_path: Option<PathBuf>,
    pub(super) max_scroll: Signal<f32>,
    pub(super) content_height: Signal<f32>,
    pub(super) scroll_y: Signal<f32>,
    pub(super) draft: Signal<String>,
    pub(super) caret: Signal<usize>,
    pub(super) editor: TextEditState,
    pub(super) editor_focus: Signal<u64>,
    recalled: Signal<Option<(usize, String, usize)>>,
    cleared: Signal<Option<(String, Vec<String>)>>,
    pub(super) status: Signal<String>,
    pub(super) notice: Signal<Option<Notice>>,
    pub(super) settings_open: Signal<bool>,
    pub(super) settings_group: Signal<Option<settings::Group>>,
    pub(super) context_open: Signal<bool>,
    pub(super) cache_open: Signal<bool>,
    pub(crate) held: Signal<Option<String>>,
    pub(super) scroll: Signal<usize>,
    pub(super) settings: super::ui::List,
    pub(super) theme_editor: super::settings::theme::Editor,
    pub(super) status_list: super::ui::List,
    pub(super) status_detail: Signal<Option<String>>,
    pub(super) modules: Signal<std::collections::BTreeMap<summonr_agent::modules::ModuleId, bool>>,
    pub(super) ticks: Signal<u64>,
    pub(super) splash_revealed: Signal<bool>,
    pub(super) splash_since: Rc<Cell<std::time::Instant>>,
    pub(super) splash: Signal<bool>,
    pub(super) sky: Signal<u64>,
    pub(super) sky_clock: Signal<Option<Signal<f64>>>,
    pub(super) settle: Signal<bool>,
    pub(super) appearance: Signal<theme::Palette>,
    pub(super) viewport: Signal<(u16, u16)>,
    pub(super) layout: Signal<Layout>,
    pub(super) pane_focus: Signal<bool>,
    pub(super) pane_lists: [super::ui::List; panes::PANES],
    pub(super) hunk_list: super::ui::List,
    pub(super) last_pane: Signal<Option<Pane>>,
    pub(super) diff_view: Signal<Option<String>>,
    pub(super) diff_root: Signal<Option<(PathBuf, String)>>,
    pub(super) note_target: Signal<Option<(String, String)>>,
    pub(super) note_text: Signal<String>,
    pub(super) note_caret: Signal<usize>,
    pub(super) view_from: Signal<Option<Pane>>,
    pub(super) diff_recent: Signal<bool>,
    pub(super) diff_session: Signal<bool>,
    pub(super) options_menu: Signal<bool>,
    pub(super) revert_armed: Confirm<String>,
    pub(super) worktree_discard: Confirm<PathBuf>,
    pub(super) view_edit: Signal<bool>,
    pub(super) view_text: Signal<String>,
    pub(super) view_caret: Signal<usize>,
    view_saved: Signal<String>,
    discard_armed: Confirm<()>,
    pub(super) copy: Signal<Option<String>>,
    pub(super) approval: Signal<Option<Asked>>,
    pub(super) form: Signal<Option<FormState>>,
    pub(super) secret: Signal<Option<(String, usize)>>,
    pub(super) secret_dialog: Signal<bool>,
    pending: Rc<
        RefCell<
            Option<(
                summonr_agent::askpass::Request,
                summonr_agent::askpass::Secret,
            )>,
        >,
    >,
    pub(super) select: Signal<bool>,
    copy_pending: Signal<Option<(Option<PathBuf>, String)>>,
    pub(super) open_file: Signal<Option<PathBuf>>,
    pub(super) quit: Signal<bool>,
    quit_armed: Confirm<()>,
    escape_armed: Confirm<()>,
    pub(super) editor_requested: Signal<bool>,
    pub(super) commands: Picker,
    pub(super) sessions: super::sessions::Picker,
    pub(super) panels: panels::Panels,
    pub(super) forge: forge::Forge,
    pub(super) usage: super::usage::Panel,
    pub(super) effort: super::effort::Strip,
    pub(super) picker: ModelPicker,
    pub(super) workflow: WorkflowDialog,
    pub(super) auth: AuthDialog,
    pub(super) hotkeys: HotkeyDialog,
    attachments: Rc<RefCell<Vec<Attachment>>>,
    pub(super) attached: Signal<usize>,
    pasted_text: Rc<RefCell<Vec<String>>>,
}

pub(super) struct Attachment {
    pub(super) image: summonr_agent::provider::Image,
    pub(super) source: Option<ImageSource>,
    pub(super) label: String,
}

impl Attachment {
    fn new(image: summonr_agent::provider::Image, source: ImageSource, label: String) -> Self {
        Self {
            image,
            source: Some(source),
            label,
        }
    }

    fn restore(image: summonr_agent::provider::Image) -> Self {
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
        let mut initial = Model::new(&info);
        initial.workspace = workspace.clone();
        let model = signal(Rc::new(initial));
        let picker = ModelPicker::new();
        let draft = signal(String::new());
        let caret = signal(0);
        let app = Self {
            inline: info.inline,
            model,
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
            status: signal(String::new()),
            notice: signal(None),
            settings_open: signal(false),
            settings_group: signal(None),
            context_open: signal(false),
            cache_open: signal(false),
            held: signal(None),
            scroll: signal(0),
            settings: super::ui::List::new(),
            theme_editor: super::settings::theme::Editor::new(),
            status_list: super::ui::List::new(),
            status_detail: signal(None),
            modules: signal({
                let switches = summonr_agent::modules::load(&workspace).ok();
                summonr_agent::modules::ALL
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
            sky: signal(super::scene::sky_seed()),
            sky_clock: signal(None),
            settle: signal(false),
            appearance: signal(theme::palette()),
            viewport: signal((80, 24)),
            layout: signal(Layout::parse(&info.prefs.layout).unwrap_or_default()),
            pane_focus: signal(false),
            pane_lists: std::array::from_fn(|_| super::ui::List::new()),
            hunk_list: super::ui::List::new(),
            last_pane: signal(None),
            diff_view: signal(None),
            diff_root: signal(None),
            note_target: signal(None),
            note_text: signal(String::new()),
            note_caret: signal(0),
            view_from: signal(None),
            diff_recent: signal(false),
            diff_session: signal(false),
            options_menu: signal(false),
            revert_armed: signal(None),
            worktree_discard: signal(None),
            view_edit: signal(false),
            view_text: signal(String::new()),
            view_caret: signal(0),
            view_saved: signal(String::new()),
            discard_armed: signal(None),
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
            quit_armed: signal(None),
            escape_armed: signal(None),
            editor_requested: signal(false),
            commands: Picker::new(draft, model, picker.facts),
            sessions: super::sessions::Picker::new(),
            panels: panels::Panels::new(),
            forge: forge::Forge::new(),
            usage: super::usage::Panel::new(),
            effort: super::effort::Strip::new(),
            picker,
            workflow: WorkflowDialog::new(),
            auth: AuthDialog::new(),
            hotkeys: HotkeyDialog::new(),
            attachments: Rc::new(RefCell::new(Vec::new())),
            attached: signal(0),
            pasted_text: Rc::new(RefCell::new(Vec::new())),
            place: signal(Place::at(&workspace)),
            window: crate::window::Window::new(),
            prefs_path: info.config.clone(),
            info,
        };
        app.cancel_secret_on_close();
        app.apply_theme();
        app.follow_backdrop();
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
