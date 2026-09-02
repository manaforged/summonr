use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sigil::prelude::{Signal, signal};
use tokio::sync::mpsc::UnboundedSender;
use unicode_width::UnicodeWidthStr;

use crate::prefs::{self, Hotkey, Prefs};
use crate::provider::UserInput;
use crate::workflow::Role;

use super::super::{Cmd, Info, Msg};
use super::canvas::cell;
use super::hit::{Hits, Target};
use super::model::{Entry, Model};
use super::pane::{Layout, Pane};
use super::suggest::Picker;

pub(super) mod presentation;

use presentation::{facts, header, settings_text, status};

#[derive(Clone)]
pub(super) struct App {
    pub(super) model: Rc<RefCell<Model>>,
    pub(super) cmd: UnboundedSender<Cmd>,
    pub(super) info: Info,
    pub(super) workspace: PathBuf,
    pub(super) git_branch: Option<String>,
    pub(super) prefs_path: Option<PathBuf>,
    pub(super) transcript: Signal<String>,
    estimated_extent: Signal<f32>,
    pub(super) extent: Signal<f32>,
    pub(super) scroll_y: Signal<f32>,
    pub(super) draft: Signal<String>,
    pub(super) caret: Signal<usize>,
    pub(super) header: Signal<String>,
    pub(super) status: Signal<String>,
    pub(super) settings: Signal<String>,
    pub(super) facts: Signal<String>,
    pub(super) panes: Signal<u64>,
    pub(super) settings_open: Signal<bool>,
    pub(super) settings_sel: Signal<usize>,
    pub(super) hotkeys_open: Signal<bool>,
    pub(super) hotkey_sel: Signal<usize>,
    pub(super) hotkey_recording: Signal<bool>,
    pub(super) hotkey_note: Signal<String>,
    pub(super) editing_model: Signal<bool>,
    pub(super) model_draft: Signal<String>,
    pub(super) model_caret: Signal<usize>,
    pub(super) model_choices: Signal<Vec<String>>,
    pub(super) model_recent: Signal<usize>,
    pub(super) model_sel: Signal<usize>,
    pub(super) model_loading: Signal<bool>,
    pub(super) model_error: Signal<String>,
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
    pub(super) hits: Hits,
    pub(super) auth_open: Signal<bool>,
    pub(super) auth_choices: Signal<Vec<crate::endpoint::AuthInfo>>,
    pub(super) auth_sel: Signal<usize>,
    pub(super) auth_entering: Signal<bool>,
    pub(super) auth_key: Signal<String>,
    pub(super) auth_caret: Signal<usize>,
    pub(super) ticks: Signal<u64>,
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
    copy_pending: Signal<bool>,
    pub(super) open_file: Signal<Option<String>>,
    pub(super) resume: Rc<RefCell<Option<String>>>,
    pub(super) quit: Signal<bool>,
    pub(super) editor_requested: Signal<bool>,
    pub(super) commands: Picker,
    pasted_images: Rc<RefCell<Vec<crate::provider::Image>>>,
}

impl App {
    pub(super) fn new(info: Info, workspace: PathBuf, cmd: UnboundedSender<Cmd>) -> Self {
        let model = Model::new(&info);
        let git_branch = read_git_branch(&workspace);
        let estimated_extent = signal(1.0);
        let extent = estimated_extent;
        Self {
            model: Rc::new(RefCell::new(model)),
            cmd,
            transcript: signal(String::new()),
            estimated_extent,
            extent,
            scroll_y: signal(0.0),
            draft: signal(String::new()),
            caret: signal(0),
            header: signal(String::new()),
            status: signal(String::new()),
            settings: signal(String::new()),
            facts: signal(String::new()),
            panes: signal(0),
            settings_open: signal(false),
            settings_sel: signal(0),
            hotkeys_open: signal(false),
            hotkey_sel: signal(0),
            hotkey_recording: signal(false),
            hotkey_note: signal(String::new()),
            editing_model: signal(false),
            model_draft: signal(String::new()),
            model_caret: signal(0),
            model_choices: signal(Vec::new()),
            model_recent: signal(0),
            model_sel: signal(0),
            model_loading: signal(false),
            model_error: signal(String::new()),
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
            hits: Hits::default(),
            auth_open: signal(false),
            auth_choices: signal(Vec::new()),
            auth_sel: signal(0),
            auth_entering: signal(false),
            auth_key: signal(String::new()),
            auth_caret: signal(0),
            ticks: signal(0),
            splash_revealed: signal(!info.prefs.animations),
            splash: signal(info.prefs.splash),
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
            copy_pending: signal(false),
            open_file: signal(None),
            resume: Rc::new(RefCell::new(None)),
            quit: signal(false),
            editor_requested: signal(false),
            commands: Picker::new(),
            pasted_images: Rc::new(RefCell::new(Vec::new())),
            git_branch,
            workspace,
            prefs_path: info.config.clone(),
            info,
        }
    }

    pub(super) fn update(&self, msg: Msg) {
        if matches!(msg, Msg::Event(crate::Event::Work { .. })) {
            match self.layout.peek().secondary {
                Some(Pane::Files) => drop(self.cmd.send(Cmd::ListFiles)),
                Some(Pane::Diff) => drop(self.cmd.send(Cmd::Diff(self.root()))),
                Some(Pane::Worktrees) => drop(self.cmd.send(Cmd::Trees)),
                _ => {}
            }
        }
        if matches!(msg, Msg::Diff(_))
            && let Some(path) = self.diff_view.peek()
        {
            drop(self.cmd.send(Cmd::Patch(self.root(), path)));
        }
        if let Msg::Patch { path, text } = &msg
            && path.is_empty()
            && self.copy_pending.peek()
        {
            self.copy_pending.set(false);
            self.copy.set(Some(text.clone()));
            return;
        }
        if let Msg::Changed(paths) = &msg {
            let viewing = self
                .model
                .borrow()
                .view
                .as_ref()
                .map(|view| view.path.clone());
            match self.layout.peek().secondary {
                Some(Pane::Files) => drop(self.cmd.send(Cmd::ListFiles)),
                Some(Pane::Diff) => drop(self.cmd.send(Cmd::Diff(self.root()))),
                Some(Pane::View) => {
                    if let Some(path) = viewing.filter(|path| paths.contains(path)) {
                        drop(self.cmd.send(Cmd::View(self.root(), path)));
                    }
                }
                _ => {}
            }
        }
        match msg {
            Msg::Resume(id) => self.request_resume(id),
            Msg::Models { mut rows, recent } => {
                let snapshot = self.model.borrow().snapshot.clone();
                let current = format!("{}\t{}", snapshot.endpoint, snapshot.model);
                let mut recent = recent.min(rows.len());
                if !rows
                    .iter()
                    .any(|row| row == &current || row == &snapshot.model)
                {
                    rows.insert(0, current);
                    recent = recent.saturating_add(1);
                }
                let selected = self
                    .model_choices
                    .peek()
                    .get(self.model_sel.peek())
                    .cloned();
                let sel = selected
                    .and_then(|row| rows.iter().position(|candidate| *candidate == row))
                    .unwrap_or(0);
                self.model_choices.set(rows);
                self.model_recent.set(recent);
                self.model_sel.set(sel);
                self.model_loading.set(false);
                self.model_error.set(String::new());
            }
            Msg::ModelsFailed(error) => {
                self.model_loading.set(false);
                self.model_error.set(error);
            }
            Msg::Workflow { name, names, seats } => {
                self.workflow_name.set(name.unwrap_or_default());
                self.workflow_names.set(names);
                let mut seats = seats;
                seats.resize(Role::ALL.len(), None);
                self.workflow_seats.set(seats);
            }
            msg => {
                if !self.model.borrow_mut().apply(msg) {
                    self.quit.set(true);
                }
            }
        }
    }

    pub(super) fn request_resume(&self, id: String) {
        *self.resume.borrow_mut() = Some(id);
        self.quit.set(true);
    }

    pub(super) fn refresh(&self) {
        let old_extent = self.extent.peek();
        let pinned = self.scroll_y.peek() >= old_extent;
        let model = self.model.borrow();
        let transcript = model.transcript();
        let width = usize::from(self.chat_cols());
        let content_left = if width > 28 {
            2
        } else {
            usize::from(width > 2)
        };
        let content_width = width.saturating_sub(content_left * 2).max(1);
        let wrapped: usize = transcript
            .lines()
            .map(|line| UnicodeWidthStr::width(line).max(1).div_ceil(content_width))
            .sum();
        self.estimated_extent
            .set(wrapped as f32 * cell().h + cell().h);
        self.transcript.set(transcript);
        self.header.set(header(&model, &self.info));
        self.status.set(status(&model, &self.info));
        self.settings
            .set(settings_text(&model.snapshot, &model.prefs));
        self.facts.set(facts(&model, &self.info));
        self.theme_name.set(model.prefs.theme.clone());
        self.panes.set(self.panes.peek().wrapping_add(1));
        if old_extent <= 1.0 || pinned {
            self.scroll_y.set(self.extent.peek());
        }
    }

    pub(super) fn refresh_status(&self) {
        self.status.set(status(&self.model.borrow(), &self.info));
    }

    pub(super) fn resize(&self, cols: u16, rows: u16) {
        self.viewport.set((cols, rows));
    }

    pub(super) fn tick(&self) {
        let busy = {
            let mut model = self.model.borrow_mut();
            let busy = model.busy_since.is_some();
            if busy {
                model.spinner = (model.spinner + 1) % 10;
            }
            busy
        };
        let ticks = self.ticks.peek().saturating_add(1);
        if busy || !self.splash_revealed.peek() {
            self.ticks.set(ticks);
        }
        if !self.splash_revealed.peek() && ticks.saturating_mul(40) > 760 {
            self.splash_revealed.set(true);
        }
    }

    pub(super) fn page_history(&self, direction: f32) {
        let page = f32::from(self.viewport.peek().1.saturating_sub(5).max(1)) * cell().h;
        let next = self.scroll_y.peek() + direction * page;
        self.scroll_y.set(next.clamp(0.0, self.extent.peek()));
    }

    pub(super) fn scroll_history(&self, delta: f32) {
        let next = self.scroll_y.peek() - delta;
        self.scroll_y.set(next.clamp(0.0, self.extent.peek()));
    }

    pub(super) fn submit(&self) {
        if self.view_edit.peek() {
            self.edit_newline();
            return;
        }
        let text = self.draft.peek();
        if text.trim().is_empty() && self.pasted_images.borrow().is_empty() {
            return;
        }
        self.replace(String::new());
        if self.local(&text) {
            return;
        }
        if text.trim_start().starts_with('/') {
            drop(self.cmd.send(Cmd::Command(text)));
            return;
        }
        let Some(mut input) = self.attach(&text) else {
            return;
        };
        input.images.extend(self.take_pasted());
        if input.text.trim().is_empty() && input.images.is_empty() {
            return;
        }
        self.splash.set(false);
        self.model
            .borrow_mut()
            .entries
            .push(Entry::User(text.clone()));
        let command = if self.model.borrow().busy_since.is_some() {
            Cmd::Steer(input)
        } else {
            Cmd::Submit(input)
        };
        drop(self.cmd.send(command));
        self.refresh();
    }

    pub(super) fn complete(&self) -> bool {
        if self.settings_open.peek() || self.model.borrow().busy_since.is_some() {
            return false;
        }
        let typed = self.draft.peek();
        let Some(name) = self.commands.complete(&typed) else {
            return false;
        };
        self.select(name);
        true
    }

    pub(super) fn accept(&self) -> bool {
        if self.settings_open.peek() || self.model.borrow().busy_since.is_some() {
            return false;
        }
        let typed = self.draft.peek();
        let Some(name) = self.commands.accept(&typed) else {
            return false;
        };
        self.select(name);
        true
    }

    pub(super) fn suggesting(&self) -> bool {
        !self.settings_open.peek()
            && !self.workflow_open.peek()
            && !self.editing_model.peek()
            && self.model.borrow().busy_since.is_none()
            && self.commands.open(&self.draft.peek())
    }

    pub(super) fn shift(&self, delta: isize) -> bool {
        self.commands.shift(&self.draft.peek(), delta)
    }

    pub(super) fn dismiss(&self) -> bool {
        self.commands.hide(&self.draft.peek())
    }

    pub(super) fn queue(&self) {
        self.splash.set(false);
        let text = self.draft.peek();
        if text.trim().is_empty() && self.pasted_images.borrow().is_empty() {
            return;
        }
        self.replace(String::new());
        let Some(mut input) = self.attach(&text) else {
            return;
        };
        input.images.extend(self.take_pasted());
        let mut model = self.model.borrow_mut();
        model.entries.push(Entry::User(text.clone()));
        model
            .entries
            .push(Entry::System("queued for the next turn".to_string()));
        drop(model);
        drop(self.cmd.send(Cmd::Queue(input)));
        self.refresh();
    }

    pub(super) fn queue_if_busy(&self) -> bool {
        if self.model.borrow().busy_since.is_none() || self.draft.peek().trim().is_empty() {
            return false;
        }
        self.queue();
        true
    }

    pub(super) fn replace(&self, draft: String) {
        self.commands.change(&draft);
        self.caret.set(draft.len());
        self.draft.set(draft);
    }

    pub(super) fn change(&self, draft: &str) {
        self.commands.change(draft);
    }

    fn select(&self, draft: &str) {
        self.caret.set(draft.len());
        self.draft.set(draft.to_string());
    }

    pub(super) fn newline(&self) {
        let mut draft = self.draft.peek();
        let mut caret = self.caret.peek().min(draft.len());
        while caret > 0 && !draft.is_char_boundary(caret) {
            caret -= 1;
        }
        draft.insert(caret, '\n');
        self.commands.change(&draft);
        self.caret.set(caret + 1);
        self.draft.set(draft);
    }

    fn attach(&self, text: &str) -> Option<UserInput> {
        let input = if text.trim_start().starts_with('!') {
            Ok(text.into())
        } else {
            crate::tools::attach(&self.workspace, text)
        };
        match input {
            Ok(input) => Some(input),
            Err(error) => {
                self.model
                    .borrow_mut()
                    .entries
                    .push(Entry::Error(format!("cannot attach files: {error}")));
                self.refresh();
                None
            }
        }
    }

    fn take_pasted(&self) -> Vec<crate::provider::Image> {
        std::mem::take(&mut *self.pasted_images.borrow_mut())
    }

    pub(super) fn paste(&self, text: &str) -> bool {
        if self.auth_open.peek() {
            return false;
        }
        let Some(path) = image_path(&self.workspace, text) else {
            return false;
        };
        let Ok(meta) = std::fs::metadata(path.as_path()) else {
            return false;
        };
        if meta.len() > crate::tools::IMAGES_MAX_BYTES as u64 {
            return false;
        }
        let Ok(bytes) = std::fs::read(path.as_path()) else {
            return false;
        };
        if bytes.len() > crate::tools::IMAGES_MAX_BYTES {
            return false;
        }
        let Some(media_type) = crate::tools::image_type(bytes.as_slice()) else {
            return false;
        };
        self.paste_image(media_type, bytes.as_slice(), &path);
        true
    }

    fn paste_image(&self, media_type: &str, bytes: &[u8], path: &Path) {
        let image = crate::provider::Image::encode(media_type, bytes);
        self.pasted_images.borrow_mut().push(image);
        let label = format!("📷 {}", path.display());
        self.model.borrow_mut().entries.push(Entry::Image {
            bytes: bytes.to_vec(),
            label,
        });
        self.refresh();
    }

    fn local(&self, line: &str) -> bool {
        match crate::parse_command(line) {
            Some(crate::SlashCommand::Settings) => {
                self.hotkeys_open.set(false);
                self.settings_open.set(true);
            }
            Some(crate::SlashCommand::Auth | crate::SlashCommand::Login) => {
                self.begin_auth(argument(line));
            }
            Some(crate::SlashCommand::Model) => {
                let model = line.trim_start_matches("/model").trim();
                if model.is_empty() {
                    self.begin_model_edit();
                } else {
                    drop(self.cmd.send(Cmd::SetModel(model.to_string())));
                }
            }
            Some(crate::SlashCommand::Workflow) if argument(line).is_none() => {
                self.open_workflow();
            }
            Some(crate::SlashCommand::Stats) => {
                let rounds = self.model.borrow().usage.clone();
                self.model.borrow_mut().entries.push(Entry::Chart(rounds));
                self.refresh();
            }
            Some(crate::SlashCommand::Theme) => self.toggle_theme(),
            Some(crate::SlashCommand::Pane) => self.pane_command(argument(line)),
            Some(crate::SlashCommand::Layout) => self.layout_command(argument(line)),
            Some(crate::SlashCommand::Diff) => self.diff_command(argument(line)),
            Some(crate::SlashCommand::View) => self.view_command(argument(line)),
            Some(crate::SlashCommand::Bus) => self.toggle_bus(),
            _ => return false,
        }
        true
    }

    pub(super) fn toggle_mode(&self) {
        let plan = !self.model.borrow().snapshot.plan;
        self.model.borrow_mut().snapshot.plan = plan;
        drop(self.cmd.send(Cmd::SetMode(plan)));
        self.refresh();
    }

    pub(super) fn toggle_verify(&self) {
        let on = self.model.borrow().snapshot.verify.is_none();
        drop(self.cmd.send(Cmd::SetVerify(on)));
    }

    pub(super) fn adjust_rounds(&self, delta: isize) {
        let current = self.model.borrow().snapshot.max_rounds;
        let next = current.saturating_add_signed(delta).clamp(8, 512);
        drop(self.cmd.send(Cmd::SetRounds(next)));
    }

    pub(super) fn cycle_endpoint(&self) {
        let current = self.model.borrow().snapshot.endpoint.clone();
        let Some(next) = crate::endpoint::next(&current) else {
            return;
        };
        drop(self.cmd.send(Cmd::SetEndpoint(next)));
    }

    pub(super) fn move_setting(&self, delta: isize) {
        if self.editing_model.peek() {
            return;
        }
        self.settings_sel.set(
            self.settings_sel
                .peek()
                .saturating_add_signed(delta)
                .min(12),
        );
    }

    pub(super) fn adjust_setting(&self, direction: isize) {
        if self.editing_model.peek() {
            return;
        }
        match self.settings_sel.peek() {
            0 => self.toggle_mode(),
            1 => self.toggle_verify(),
            2 => self.adjust_rounds(direction * 8),
            3 => self.cycle_endpoint(),
            4 => self.begin_model_edit(),
            5 => self.open_workflow(),
            6 => self.toggle_theme(),
            7 => {
                let mut model = self.model.borrow_mut();
                model.prefs.splash = !model.prefs.splash;
                let prefs = model.prefs.clone();
                drop(model);
                self.persist(&prefs);
                self.refresh();
            }
            8 => {
                let mut model = self.model.borrow_mut();
                model.prefs.animations = !model.prefs.animations;
                let prefs = model.prefs.clone();
                self.splash_revealed.set(!model.prefs.animations);
                drop(model);
                self.persist(&prefs);
                self.refresh();
            }
            9 => self.toggle_reasoning(),
            10 => self.toggle_tools(),
            11 => self.toggle_bus(),
            12 => self.open_hotkeys(),
            _ => {}
        }
    }

    pub(super) fn toggle_options(&self) {
        if self.auth_open.peek() {
            self.cancel_auth();
            return;
        }
        if self.editing_model.peek() {
            self.cancel_model_edit();
            return;
        }
        if self.workflow_open.peek() {
            self.workflow_open.set(false);
            return;
        }
        if self.hotkeys_open.peek() {
            self.close_hotkeys();
            return;
        }
        let open = !self.settings_open.peek();
        if open {
            self.settings_sel.set(0);
        }
        self.settings_open.set(open);
    }

    pub(super) fn open_hotkeys(&self) {
        self.hotkey_sel.set(0);
        self.hotkey_recording.set(false);
        self.hotkey_note.set(String::new());
        self.hotkeys_open.set(true);
    }

    pub(super) fn close_hotkeys(&self) {
        self.hotkey_recording.set(false);
        self.hotkey_note.set(String::new());
        self.hotkeys_open.set(false);
    }

    pub(super) fn move_hotkey(&self, delta: isize) {
        if self.hotkey_recording.peek() {
            return;
        }
        self.hotkey_sel.set(
            self.hotkey_sel
                .peek()
                .saturating_add_signed(delta)
                .min(Hotkey::ALL.len() - 1),
        );
    }

    pub(super) fn edit_hotkey(&self) {
        if !self.hotkey_recording.peek() {
            self.hotkey_note.set(String::new());
            self.hotkey_recording.set(true);
        }
    }

    pub(super) fn reset_hotkeys(&self) {
        let mut model = self.model.borrow_mut();
        model.prefs.hotkeys = Default::default();
        let prefs = model.prefs.clone();
        drop(model);
        self.hotkey_recording.set(false);
        self.hotkey_note.set(String::new());
        self.persist(&prefs);
        self.refresh();
    }

    pub(super) fn route_hotkey(&self, key: char) -> bool {
        if self.hotkey_recording.peek() {
            if !crate::prefs::available(key) {
                self.hotkey_note.set(format!("ctrl+{key} is reserved"));
                return true;
            }
            let action = Hotkey::ALL[self.hotkey_sel.peek().min(Hotkey::ALL.len() - 1)];
            let mut model = self.model.borrow_mut();
            let _ = model.prefs.hotkeys.assign(action, key);
            let prefs = model.prefs.clone();
            drop(model);
            self.hotkey_recording.set(false);
            self.hotkey_note.set(String::new());
            self.persist(&prefs);
            self.refresh();
            return true;
        }
        let action = self.model.borrow().prefs.hotkeys.action(key);
        let Some(action) = action else {
            return false;
        };
        if action != Hotkey::Options && self.options_active() {
            return true;
        }
        match action {
            Hotkey::Options => self.toggle_options(),
            Hotkey::Newline => self.newline(),
            Hotkey::Editor => self.editor_requested.set(true),
            Hotkey::Files => self.toggle_pane(Pane::Files),
            Hotkey::Ledger => self.toggle_pane(Pane::Ledger),
            Hotkey::Diff => self.toggle_pane(Pane::Diff),
            Hotkey::Pane => self.toggle_side(),
        }
        true
    }

    pub(super) fn options_active(&self) -> bool {
        self.settings_open.peek()
            || self.hotkeys_open.peek()
            || self.auth_open.peek()
            || self.editing_model.peek()
            || self.workflow_open.peek()
    }

    pub(super) fn press(&self, col: usize, row: usize) -> bool {
        let Some(target) = self.hits.at(col, row) else {
            return false;
        };
        if let Target::Role(index) = target
            && self.workflow_open.peek()
            && !self.workflow_naming.peek()
        {
            self.workflow_sel.set(index);
            self.drag_from.set(Some(index));
            self.drag_over.set(Some(index));
            return true;
        }
        self.click(target);
        false
    }

    pub(super) fn drag_to(&self, col: usize, row: usize) {
        if self.drag_from.peek().is_none() {
            return;
        }
        self.drag_over.set(match self.hits.at(col, row) {
            Some(Target::Role(index)) => Some(index),
            _ => None,
        });
    }

    pub(super) fn release(&self, col: usize, row: usize) {
        let Some(from) = self.drag_from.peek() else {
            return;
        };
        self.drag_from.set(None);
        self.drag_over.set(None);
        match self.hits.at(col, row) {
            Some(Target::Role(to)) if to == from => self.click(Target::Role(to)),
            Some(Target::Role(to)) => self.copy_seat(from, to),
            _ => {}
        }
    }

    fn copy_seat(&self, from: usize, to: usize) {
        let Some(role) = Role::ALL.get(to) else {
            return;
        };
        let seat = self.workflow_seats.peek().get(from).cloned().flatten();
        let command = match seat.and_then(|seat| {
            seat.split_once('\t')
                .map(|(endpoint, model)| (endpoint.to_string(), model.to_string()))
        }) {
            Some((endpoint, model)) => Cmd::SeatRole(*role, endpoint, model),
            None => Cmd::ClearRole(*role),
        };
        self.workflow_sel.set(to);
        drop(self.cmd.send(command));
    }

    fn click(&self, target: Target) {
        match target {
            Target::Setting(index) => {
                if self.settings_sel.peek() == index {
                    self.adjust_setting(1);
                } else {
                    self.settings_sel.set(index);
                }
            }
            Target::Hotkey(index) => {
                self.hotkey_recording.set(false);
                self.hotkey_note.set(String::new());
                self.hotkey_sel.set(index);
                self.edit_hotkey();
            }
            Target::Model(index) => {
                self.model_sel.set(index);
                self.commit_model_edit();
            }
            Target::Auth(index) => {
                self.auth_sel.set(index);
                self.commit_auth();
            }
            Target::Role(index) => {
                self.workflow_sel.set(index);
                self.pick_workflow_seat();
            }
            Target::File(index) => {
                self.file_cursor.set(index);
                self.pane_focus.set(true);
                self.pane_enter();
            }
            Target::Close => self.toggle_side(),
            Target::Saved(name) => {
                drop(self.cmd.send(Cmd::Command(format!("/workflow {name}"))));
            }
        }
    }

    pub(super) fn open_workflow(&self) {
        self.workflow_sel.set(0);
        self.workflow_naming.set(false);
        self.replace_workflow_draft(String::new());
        self.workflow_open.set(true);
        drop(self.cmd.send(Cmd::ListWorkflows));
    }

    pub(super) fn close_workflow(&self) {
        if self.workflow_naming.peek() {
            self.workflow_naming.set(false);
            self.replace_workflow_draft(String::new());
        } else {
            self.workflow_open.set(false);
        }
    }

    pub(super) fn move_workflow(&self, delta: isize) {
        if self.workflow_naming.peek() {
            return;
        }
        let last = Role::ALL.len().saturating_sub(1);
        self.workflow_sel.set(
            self.workflow_sel
                .peek()
                .saturating_add_signed(delta)
                .min(last),
        );
    }

    pub(super) fn cycle_workflow(&self, delta: isize) {
        if self.workflow_naming.peek() {
            return;
        }
        let names = self.workflow_names.peek();
        if names.is_empty() {
            return;
        }
        let current = self.workflow_name.peek();
        let index = names.iter().position(|name| *name == current);
        let next = match (index, delta.is_negative()) {
            (Some(index), _) => (index as isize + delta).rem_euclid(names.len() as isize) as usize,
            (None, true) => names.len() - 1,
            (None, false) => 0,
        };
        drop(
            self.cmd
                .send(Cmd::Command(format!("/workflow {}", names[next]))),
        );
    }

    pub(super) fn pick_workflow_seat(&self) {
        if self.workflow_naming.peek() {
            self.commit_workflow();
            return;
        }
        self.model_for.set(Some(self.workflow_sel.peek()));
        self.begin_model_edit();
    }

    pub(super) fn clear_workflow_seat(&self) {
        if self.workflow_naming.peek() {
            return;
        }
        if let Some(role) = Role::ALL.get(self.workflow_sel.peek()) {
            drop(self.cmd.send(Cmd::ClearRole(*role)));
        }
    }

    pub(super) fn begin_workflow_save(&self) {
        if self.workflow_naming.peek() {
            return;
        }
        self.replace_workflow_draft(self.workflow_name.peek());
        self.workflow_naming.set(true);
    }

    pub(super) fn commit_workflow(&self) {
        let name = self.workflow_draft.peek().trim().to_string();
        if name.is_empty() {
            return;
        }
        self.workflow_naming.set(false);
        self.replace_workflow_draft(String::new());
        drop(self.cmd.send(Cmd::SaveWorkflow(name)));
    }

    pub(super) fn replace_workflow_draft(&self, draft: String) {
        self.workflow_caret.set(draft.len());
        self.workflow_draft.set(draft);
    }

    pub(super) fn begin_model_edit(&self) {
        self.replace_model_draft(String::new());
        let snapshot = self.model.borrow().snapshot.clone();
        self.model_choices
            .set(vec![format!("{}\t{}", snapshot.endpoint, snapshot.model)]);
        self.model_sel.set(0);
        self.model_error.set(String::new());
        self.model_loading.set(true);
        self.editing_model.set(true);
        drop(self.cmd.send(Cmd::ListModels));
    }

    pub(super) fn begin_auth(&self, focus: Option<&str>) {
        let choices: Vec<_> = crate::endpoint::names()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|name| crate::endpoint::auth_info(&name).ok())
            .collect();
        let current = self.model.borrow().snapshot.endpoint.clone();
        let focus = focus.unwrap_or(&current);
        let selected = choices
            .iter()
            .position(|choice| choice.name == focus)
            .or_else(|| choices.iter().position(|choice| choice.name == current))
            .unwrap_or(0);
        self.auth_choices.set(choices);
        self.auth_sel.set(selected);
        self.auth_entering.set(false);
        self.replace_auth_key(String::new());
        self.auth_open.set(true);
    }

    pub(super) fn move_auth(&self, delta: isize) {
        if self.auth_entering.peek() {
            return;
        }
        let last = self.auth_choices.peek().len().saturating_sub(1);
        self.auth_sel
            .set(self.auth_sel.peek().saturating_add_signed(delta).min(last));
    }

    pub(super) fn commit_auth(&self) {
        let choices = self.auth_choices.peek();
        let Some(choice) = choices.get(self.auth_sel.peek().min(choices.len().saturating_sub(1)))
        else {
            return;
        };
        if choice.device_login {
            self.auth_open.set(false);
            drop(self.cmd.send(Cmd::Login(choice.name.clone())));
            return;
        }
        if !self.auth_entering.peek() {
            self.auth_entering.set(true);
            self.replace_auth_key(String::new());
            return;
        }
        let key = self.auth_key.peek();
        if key.trim().is_empty() {
            return;
        }
        self.auth_open.set(false);
        self.auth_entering.set(false);
        self.replace_auth_key(String::new());
        drop(self.cmd.send(Cmd::SetKey(choice.name.clone(), key)));
    }

    pub(super) fn cancel_auth(&self) {
        if self.auth_entering.peek() {
            self.auth_entering.set(false);
            self.replace_auth_key(String::new());
        } else {
            self.auth_open.set(false);
        }
    }

    fn replace_auth_key(&self, key: String) {
        self.auth_caret.set(key.len());
        self.auth_key.set(key);
    }

    pub(super) fn replace_model_draft(&self, draft: String) {
        self.model_caret.set(draft.len());
        self.model_draft.set(draft);
    }

    pub(super) fn commit_model_edit(&self) {
        let choices = self.filtered_models();
        let draft = self.model_draft.peek();
        let selected = self.model_sel.peek().min(choices.len().saturating_sub(1));
        let picked = choices
            .get(selected)
            .cloned()
            .unwrap_or_else(|| draft.trim().to_string());
        if picked.trim().is_empty() {
            return;
        }
        self.editing_model.set(false);
        let target = self.model_for.peek();
        self.model_for.set(None);
        if let Some(role) = target.and_then(|index| Role::ALL.get(index)) {
            let current = self.model.borrow().snapshot.endpoint.clone();
            let (endpoint, model) = picked
                .split_once('\t')
                .map_or((current, picked.clone()), |(name, model)| {
                    (name.to_string(), model.to_string())
                });
            drop(self.cmd.send(Cmd::SeatRole(*role, endpoint, model)));
            return;
        }
        if let Some((name, model)) = picked.split_once('\t') {
            let current = self.model.borrow().snapshot.endpoint.clone();
            if name != current {
                drop(
                    self.cmd
                        .send(Cmd::SetSeat(name.to_string(), model.to_string())),
                );
            } else {
                drop(self.cmd.send(Cmd::SetModel(model.to_string())));
            }
        } else {
            drop(self.cmd.send(Cmd::SetModel(picked)));
        }
    }

    pub(super) fn cancel_model_edit(&self) {
        self.editing_model.set(false);
        self.model_for.set(None);
        self.replace_model_draft(String::new());
    }

    pub(super) fn move_model(&self, delta: isize) {
        let last = self.filtered_models().len().saturating_sub(1);
        self.model_sel
            .set(self.model_sel.peek().saturating_add_signed(delta).min(last));
    }

    pub(super) fn filtered_models(&self) -> Vec<String> {
        let query = self.model_draft.peek().to_lowercase();
        self.model_choices
            .peek()
            .into_iter()
            .filter(|model| query.is_empty() || model.to_lowercase().contains(&query))
            .collect()
    }

    pub(super) fn chat_cols(&self) -> u16 {
        self.layout.get().split(self.viewport.get().0).0
    }

    pub(super) fn set_layout(&self, layout: Layout) {
        if self.view_edit.peek() && layout.secondary != Some(Pane::View) {
            if self.view_text.peek() != self.view_saved.peek() {
                self.model.borrow_mut().entries.push(Entry::Error(
                    "unsaved edits in the view pane — ctrl+s saves, esc leaves".to_string(),
                ));
                self.refresh();
                return;
            }
            self.view_edit.set(false);
        }
        let opened = |pane: Pane| {
            layout.secondary == Some(pane) && self.layout.peek().secondary != Some(pane)
        };
        if opened(Pane::Files) {
            drop(self.cmd.send(Cmd::ListFiles));
        }
        if opened(Pane::Diff) {
            drop(self.cmd.send(Cmd::Diff(self.root())));
        }
        if opened(Pane::Worktrees) {
            drop(self.cmd.send(Cmd::Trees));
        }
        if layout.secondary.is_none() {
            self.pane_focus.set(false);
        }
        self.layout.set(layout);
        let mut model = self.model.borrow_mut();
        model.prefs.layout = layout.text();
        model
            .entries
            .push(Entry::System(format!("layout: {}", layout.text())));
        let prefs = model.prefs.clone();
        drop(model);
        self.persist(&prefs);
        self.refresh();
    }

    pub(super) fn toggle_pane(&self, pane: Pane) {
        if pane == Pane::Diff {
            self.diff_root.set(None);
        }
        let layout = self.layout.peek();
        if layout.secondary == Some(pane) {
            self.last_pane.set(Some(pane));
            self.set_layout(Layout {
                secondary: None,
                ..layout
            });
        } else {
            self.set_layout(Layout {
                secondary: Some(pane),
                ..layout
            });
            self.pane_focus.set(true);
        }
    }

    pub(super) fn toggle_side(&self) {
        let layout = self.layout.peek();
        match layout.secondary {
            Some(pane) => self.toggle_pane(pane),
            None => {
                let pane = self.last_pane.peek().unwrap_or(Pane::Files);
                self.toggle_pane(pane);
            }
        }
    }

    pub(super) fn pane_page(&self, delta: isize) {
        let rows = isize::from(
            i16::try_from(self.viewport.peek().1.saturating_sub(4).max(1)).unwrap_or(1),
        );
        self.pane_move(delta * rows);
    }

    pub(super) fn pane_edge(&self, end: bool) {
        if self.layout.peek().secondary == Some(Pane::View) {
            let last = self.view_lines().saturating_sub(1) as isize;
            self.view_move(if end { last } else { -last });
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Worktrees) {
            let last = self.tree_rows().len().saturating_sub(1) as isize;
            self.tree_move(if end { last } else { -last });
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Diff) {
            let last = if self.diff_view.peek().is_some() {
                self.patch_lines().saturating_sub(1)
            } else {
                self.diff_rows().len().saturating_sub(1)
            };
            self.diff_move(if end { last as isize } else { -(last as isize) });
            return;
        }
        let count = self.file_rows().len();
        if count > 0 {
            self.file_cursor.set(if end { count - 1 } else { 0 });
            self.refresh();
        }
    }

    pub(super) fn pane_collapse(&self) {
        if self.layout.peek().secondary == Some(Pane::View) {
            self.pane_back();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Worktrees) {
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Diff) {
            self.pane_back();
            return;
        }
        let rows = self.file_rows();
        let cursor = self.file_cursor.peek().min(rows.len().saturating_sub(1));
        let Some(row) = rows.get(cursor) else {
            return;
        };
        if row.dir && row.open {
            self.pane_enter();
            return;
        }
        if let Some((parent, _)) = row.path.rsplit_once('/')
            && let Some(index) = rows.iter().position(|candidate| candidate.path == parent)
        {
            self.file_cursor.set(index);
            self.refresh();
        }
    }

    pub(super) fn pane_expand(&self) {
        if self.layout.peek().secondary == Some(Pane::View) {
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Worktrees) {
            self.tree_enter();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Diff) {
            if self.diff_view.peek().is_none() {
                self.diff_enter();
            }
            return;
        }
        let rows = self.file_rows();
        let cursor = self.file_cursor.peek().min(rows.len().saturating_sub(1));
        match rows.get(cursor) {
            Some(row) if row.dir => {
                if !row.open {
                    self.pane_enter();
                }
            }
            Some(row) => self.view_open(row.path.clone()),
            None => {}
        }
    }

    /// Shows one workspace file in the view pane; esc returns to the pane
    /// it was opened from.
    pub(super) fn view_open(&self, path: String) {
        let layout = self.layout.peek();
        if layout.secondary != Some(Pane::View) {
            self.view_from.set(layout.secondary);
            self.set_layout(Layout {
                secondary: Some(Pane::View),
                ..layout
            });
        }
        self.view_scroll.set(0);
        self.pane_focus.set(true);
        drop(self.cmd.send(Cmd::View(self.root(), path)));
        self.refresh();
    }

    /// Turns the view pane into an editor on the file it shows.
    pub(super) fn edit_start(&self) {
        let text = self
            .model
            .borrow()
            .view
            .as_ref()
            .map(|view| view.rope.to_string_lossy());
        let Some(text) = text else {
            return;
        };
        self.view_saved.set(text.clone());
        self.view_text.set(text);
        self.view_caret.set(0);
        self.discard_armed.set(false);
        self.view_edit.set(true);
        self.pane_focus.set(true);
        self.refresh();
    }

    /// Leaves edit mode. Unsaved text needs a second call, so one esc cannot
    /// discard work. Returns false when nothing was being edited.
    pub(super) fn edit_stop(&self) -> bool {
        if !self.view_edit.peek() {
            return false;
        }
        if self.view_text.peek() != self.view_saved.peek() && !self.discard_armed.peek() {
            self.discard_armed.set(true);
            self.model.borrow_mut().entries.push(Entry::System(
                "unsaved edits — ctrl+s saves, esc again discards".to_string(),
            ));
            self.refresh();
            return true;
        }
        self.view_edit.set(false);
        self.discard_armed.set(false);
        self.refresh();
        true
    }

    pub(super) fn edit_save(&self) {
        let path = self
            .model
            .borrow()
            .view
            .as_ref()
            .map(|view| view.path.clone());
        let Some(path) = path else {
            return;
        };
        let text = self.view_text.peek();
        self.view_saved.set(text.clone());
        self.discard_armed.set(false);
        drop(self.cmd.send(Cmd::Save(path, text)));
        self.refresh();
    }

    fn edit_newline(&self) {
        let mut text = self.view_text.peek();
        let caret = self.view_caret.peek().min(text.len());
        if !text.is_char_boundary(caret) {
            return;
        }
        text.insert(caret, '\n');
        self.view_text.set(text);
        self.view_caret.set(caret + 1);
        self.refresh();
    }

    pub(super) fn view_external(&self) {
        if let Some(path) = self
            .model
            .borrow()
            .view
            .as_ref()
            .map(|view| view.path.clone())
        {
            self.open_file.set(Some(path));
        }
        self.refresh();
    }

    fn view_lines(&self) -> usize {
        self.model
            .borrow()
            .view
            .as_ref()
            .map_or(0, |view| view.rope.to_string_lossy().lines().count())
    }

    fn view_move(&self, delta: isize) {
        let last = self.view_lines().saturating_sub(1) as isize;
        let scroll = (self.view_scroll.peek() as isize + delta).clamp(0, last);
        self.view_scroll.set(scroll as usize);
        self.refresh();
    }

    fn view_command(&self, argument: Option<&str>) {
        match argument {
            Some(path) => self.view_open(path.trim_start_matches('@').to_string()),
            None => {
                let path = self
                    .model
                    .borrow()
                    .view
                    .as_ref()
                    .map(|view| view.path.clone());
                match path {
                    Some(path) => self.view_open(path),
                    None => {
                        self.model
                            .borrow_mut()
                            .entries
                            .push(Entry::Error("usage: /view PATH".to_string()));
                        self.refresh();
                    }
                }
            }
        }
    }

    pub(super) fn context_at(&self, col: usize, row: usize) -> bool {
        let Some(Target::File(index)) = self.hits.at(col, row) else {
            return false;
        };
        let kind = self.layout.peek().secondary.unwrap_or(Pane::Files);
        let rows = match kind {
            Pane::Diff => self.diff_rows(),
            Pane::Worktrees => self.tree_rows(),
            Pane::View => return false,
            _ => self.file_rows(),
        };
        let Some(target) = rows.get(index).cloned() else {
            return false;
        };
        match kind {
            Pane::Diff => self.diff_cursor.set(index),
            Pane::Worktrees => self.tree_cursor.set(index),
            _ => self.file_cursor.set(index),
        }
        self.pane_focus.set(true);
        self.context.set(Some(Context {
            col,
            row,
            cursor: 0,
            target,
            kind,
        }));
        self.refresh();
        true
    }

    pub(super) fn context_move(&self, delta: isize) {
        let Some(mut context) = self.context.peek() else {
            return;
        };
        let count = context.items().len() as isize;
        context.cursor = ((context.cursor as isize + delta).rem_euclid(count)) as usize;
        self.context.set(Some(context));
        self.refresh();
    }

    pub(super) fn context_close(&self) {
        self.context.set(None);
        self.refresh();
    }

    pub(super) fn context_enter(&self) {
        let Some(context) = self.context.peek() else {
            return;
        };
        self.context.set(None);
        match context
            .items()
            .get(context.cursor)
            .map(|(_, action)| *action)
        {
            Some(Action::Toggle) => self.pane_enter(),
            Some(Action::Attach) => self.mention(&context.target.path),
            Some(Action::Hunks) => self.diff_open(context.target.path.clone()),
            Some(Action::View) => self.view_open(context.target.path.clone()),
            Some(Action::Diff) => {
                self.tree_show(
                    PathBuf::from(&context.target.path),
                    context.target.name.clone(),
                );
            }
            Some(Action::Merge) => {
                drop(
                    self.cmd
                        .send(Cmd::Merge(PathBuf::from(&context.target.path))),
                );
                self.refresh();
            }
            Some(Action::Discard) => {
                drop(
                    self.cmd
                        .send(Cmd::Discard(PathBuf::from(&context.target.path))),
                );
                self.refresh();
            }
            Some(Action::Open) => {
                self.open_file.set(Some(context.target.path.clone()));
                self.refresh();
            }
            Some(Action::Insert) => {
                let mut draft = self.draft.peek();
                draft.push_str(&context.target.path);
                self.replace(draft);
                self.pane_focus.set(false);
                self.refresh();
            }
            None => self.refresh(),
        }
    }

    pub(super) fn focus_pane(&self, on: bool) {
        let has_pane = self.layout.peek().secondary.is_some();
        self.pane_focus.set(on && has_pane);
        self.refresh();
    }

    pub(super) fn pane_active(&self) -> Option<Pane> {
        if self.view_edit.peek() {
            return None;
        }
        self.pane_focus
            .peek()
            .then(|| self.layout.peek().secondary)
            .flatten()
    }

    pub(super) fn file_rows(&self) -> Vec<FileRow> {
        let model = self.model.borrow();
        let expanded = self.expanded.peek();
        file_rows(&model.files, &expanded)
    }

    pub(super) fn pane_move(&self, delta: isize) {
        if self.layout.peek().secondary == Some(Pane::View) {
            self.view_move(delta);
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Worktrees) {
            self.tree_move(delta);
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Diff) {
            self.diff_move(delta);
            return;
        }
        let count = self.file_rows().len();
        if count == 0 {
            return;
        }
        let cursor = self.file_cursor.peek().min(count - 1) as isize;
        let next = (cursor + delta).clamp(0, count as isize - 1) as usize;
        self.file_cursor.set(next);
        self.refresh();
    }

    pub(super) fn pane_enter(&self) {
        if self.layout.peek().secondary == Some(Pane::View) {
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Worktrees) {
            self.tree_enter();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Diff) {
            self.diff_enter();
            return;
        }
        let rows = self.file_rows();
        let Some(row) = rows.get(self.file_cursor.peek().min(rows.len().saturating_sub(1))) else {
            return;
        };
        if row.dir {
            let mut expanded = self.expanded.peek();
            match expanded.iter().position(|dir| *dir == row.path) {
                Some(index) => drop(expanded.remove(index)),
                None => expanded.push(row.path.clone()),
            }
            self.expanded.set(expanded);
        } else {
            self.mention(&row.path);
        }
        self.refresh();
    }

    fn mention(&self, path: &str) {
        let mut draft = self.draft.peek();
        if !draft.is_empty() && !draft.ends_with(' ') {
            draft.push(' ');
        }
        draft.push('@');
        draft.push_str(path);
        draft.push(' ');
        self.replace(draft);
        self.pane_focus.set(false);
        self.refresh();
    }

    /// The diff rows on show: every change, or only the last turn's files
    /// while the `t` filter is on.
    pub(super) fn diff_stats(&self) -> Vec<crate::diff::Stat> {
        let model = self.model.borrow();
        let recent = self.diff_recent.peek();
        model
            .diff
            .files
            .iter()
            .filter(|stat| !recent || model.recent.contains(&stat.path))
            .cloned()
            .collect()
    }

    pub(super) fn toggle_recent(&self) {
        self.diff_recent.set(!self.diff_recent.peek());
        self.diff_cursor.set(0);
        self.refresh();
    }

    /// Copies the patch on show: the viewed file's hunks, or the whole
    /// workspace patch from the list.
    pub(super) fn copy_patch(&self) {
        let viewed = self.diff_view.peek();
        let text = self
            .model
            .borrow()
            .patch
            .as_ref()
            .filter(|(path, _)| viewed.as_deref() == Some(path.as_str()))
            .map(|(_, text)| text.clone());
        match text {
            Some(text) => self.copy.set(Some(text)),
            None => {
                self.copy_pending.set(true);
                drop(self.cmd.send(Cmd::Patch(self.root(), String::new())));
            }
        }
        self.refresh();
    }

    pub(super) fn copied(&self, text: &str) {
        let lines = text.lines().count();
        self.model.borrow_mut().entries.push(Entry::System(format!(
            "copied the patch: {lines} line{}",
            if lines == 1 { "" } else { "s" }
        )));
        self.refresh();
    }

    pub(super) fn diff_rows(&self) -> Vec<FileRow> {
        self.diff_stats()
            .iter()
            .map(|stat| FileRow {
                path: stat.path.clone(),
                name: stat.path.clone(),
                depth: 0,
                dir: false,
                open: false,
            })
            .collect()
    }

    fn patch_lines(&self) -> usize {
        self.model
            .borrow()
            .patch
            .as_ref()
            .map_or(0, |(_, text)| text.lines().count())
    }

    fn diff_move(&self, delta: isize) {
        if self.diff_view.peek().is_some() {
            let last = self.patch_lines().saturating_sub(1) as isize;
            let scroll = (self.diff_scroll.peek() as isize + delta).clamp(0, last);
            self.diff_scroll.set(scroll as usize);
        } else {
            let count = self.diff_rows().len();
            if count == 0 {
                return;
            }
            let cursor = self.diff_cursor.peek().min(count - 1) as isize;
            self.diff_cursor
                .set((cursor + delta).clamp(0, count as isize - 1) as usize);
        }
        self.refresh();
    }

    fn diff_enter(&self) {
        if self.pane_back() {
            return;
        }
        let rows = self.diff_rows();
        let cursor = self.diff_cursor.peek().min(rows.len().saturating_sub(1));
        if let Some(row) = rows.get(cursor) {
            self.diff_open(row.path.clone());
        }
    }

    pub(super) fn diff_open(&self, path: String) {
        self.diff_scroll.set(0);
        self.diff_view.set(Some(path.clone()));
        drop(self.cmd.send(Cmd::Patch(self.root(), path)));
        self.refresh();
    }

    /// The directory the diff pane reads: a worktree chosen in the
    /// worktrees pane, or the session workspace.
    fn root(&self) -> Option<PathBuf> {
        self.diff_root.peek().map(|(path, _)| path)
    }

    pub(super) fn tree_rows(&self) -> Vec<FileRow> {
        self.model
            .borrow()
            .trees
            .iter()
            .map(|row| FileRow {
                path: row.tree.path.display().to_string(),
                name: row.tree.branch.clone(),
                depth: 0,
                dir: false,
                open: false,
            })
            .collect()
    }

    fn tree_move(&self, delta: isize) {
        let count = self.tree_rows().len();
        if count == 0 {
            return;
        }
        let cursor = self.tree_cursor.peek().min(count - 1) as isize;
        self.tree_cursor
            .set((cursor + delta).clamp(0, count as isize - 1) as usize);
        self.refresh();
    }

    fn tree_enter(&self) {
        let rows = self.tree_rows();
        let cursor = self.tree_cursor.peek().min(rows.len().saturating_sub(1));
        if let Some(row) = rows.get(cursor) {
            self.tree_show(PathBuf::from(&row.path), row.name.clone());
        }
    }

    /// Opens the diff pane on one worktree; esc returns to the list.
    pub(super) fn tree_show(&self, path: PathBuf, branch: String) {
        self.diff_root.set(Some((path, branch)));
        self.diff_view.set(None);
        let layout = self.layout.peek();
        self.set_layout(Layout {
            secondary: Some(Pane::Diff),
            ..layout
        });
        self.pane_focus.set(true);
        self.refresh();
    }

    /// Leaves the hunk view for the file list. Returns false when there is
    /// no hunk view to leave.
    pub(super) fn pane_back(&self) -> bool {
        let layout = self.layout.peek();
        if layout.secondary == Some(Pane::View) {
            let back = self.view_from.peek().unwrap_or(Pane::Files);
            self.set_layout(Layout {
                secondary: Some(back),
                ..layout
            });
            self.pane_focus.set(true);
            self.refresh();
            return true;
        }
        if layout.secondary != Some(Pane::Diff) {
            return false;
        }
        if self.diff_view.peek().is_some() {
            self.diff_view.set(None);
            self.refresh();
            return true;
        }
        if self.diff_root.peek().is_some() {
            self.diff_root.set(None);
            self.set_layout(Layout {
                secondary: Some(Pane::Worktrees),
                ..layout
            });
            self.pane_focus.set(true);
            self.refresh();
            return true;
        }
        false
    }

    fn diff_command(&self, argument: Option<&str>) {
        self.diff_root.set(None);
        let layout = self.layout.peek();
        if layout.secondary != Some(Pane::Diff) {
            self.set_layout(Layout {
                secondary: Some(Pane::Diff),
                ..layout
            });
        }
        self.pane_focus.set(true);
        match argument {
            Some(path) => self.diff_open(path.to_string()),
            None => {
                self.diff_view.set(None);
                self.refresh();
            }
        }
    }

    pub(super) fn nudge_split(&self, delta: i16) {
        let layout = self.layout.peek();
        if layout.secondary.is_some() {
            self.set_layout(layout.nudge(delta));
        }
    }

    fn pane_command(&self, argument: Option<&str>) {
        let layout = self.layout.peek();
        match argument {
            None => {
                let names: Vec<&str> = Pane::SECONDARY.iter().map(|pane| pane.name()).collect();
                self.model.borrow_mut().entries.push(Entry::System(format!(
                    "layout: {} · /pane {} · /pane off · /layout focus|run|files|review",
                    layout.text(),
                    names.join("|")
                )));
                self.refresh();
            }
            Some("off" | "close") => self.set_layout(Layout {
                secondary: None,
                ..layout
            }),
            Some(name) => match Pane::parse(name) {
                Some(Pane::Chat) | None => {
                    self.model
                        .borrow_mut()
                        .entries
                        .push(Entry::Error(format!("no pane named `{name}`")));
                    self.refresh();
                }
                Some(pane) => self.set_layout(Layout {
                    secondary: Some(pane),
                    ..layout
                }),
            },
        }
    }

    fn layout_command(&self, argument: Option<&str>) {
        match argument {
            None => self.pane_command(None),
            Some(text) => match Layout::parse(text) {
                Some(layout) => self.set_layout(layout),
                None => {
                    self.model.borrow_mut().entries.push(Entry::Error(format!(
                        "no layout `{text}` — focus, run, files, review, or `chat | ledger 40%`"
                    )));
                    self.refresh();
                }
            },
        }
    }

    pub(super) fn toggle_theme(&self) {
        let mut model = self.model.borrow_mut();
        model.prefs.theme = if model.prefs.theme == "fantasy" {
            "space".to_string()
        } else {
            "fantasy".to_string()
        };
        let prefs = model.prefs.clone();
        drop(model);
        self.persist(&prefs);
        self.refresh();
    }

    pub(super) fn toggle_bus(&self) {
        let mut model = self.model.borrow_mut();
        model.prefs.bus = !model.prefs.bus;
        let on = model.prefs.bus;
        let prefs = model.prefs.clone();
        drop(model);
        self.persist(&prefs);
        drop(self.cmd.send(Cmd::SetBus(on)));
        self.refresh();
    }

    pub(super) fn toggle_reasoning(&self) {
        let mut model = self.model.borrow_mut();
        model.prefs.reasoning = !model.prefs.reasoning;
        let prefs = model.prefs.clone();
        drop(model);
        self.persist(&prefs);
        self.refresh();
    }

    pub(super) fn toggle_tools(&self) {
        let mut model = self.model.borrow_mut();
        model.prefs.tools = model.prefs.tools.toggle();
        let prefs = model.prefs.clone();
        drop(model);
        self.persist(&prefs);
        self.refresh();
    }

    pub(super) fn interrupt(&self) {
        if self.auth_open.peek() {
            self.cancel_auth();
        } else if self.workflow_open.peek() {
            self.close_workflow();
        } else if self.model.borrow().busy_since.is_some() {
            drop(self.cmd.send(Cmd::Cancel));
        } else if self.draft.peek().is_empty() {
            self.quit.set(true);
        } else {
            self.replace(String::new());
        }
    }

    fn persist(&self, prefs: &Prefs) {
        if let Err(error) = save(self.prefs_path.as_deref(), prefs) {
            self.model.borrow_mut().entries.push(Entry::Error(error));
        }
    }
}

fn argument(line: &str) -> Option<&str> {
    line.split_once(char::is_whitespace)
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
}

fn save(path: Option<&Path>, prefs: &Prefs) -> Result<(), String> {
    let path = path.ok_or_else(|| "settings not saved: home directory unavailable".to_string())?;
    prefs::save_in(path, prefs).map_err(|error| format!("settings not saved: {error}"))
}

fn image_path(workspace: &Path, text: &str) -> Option<PathBuf> {
    let text = text.trim();
    let text = text
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            text.strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .or_else(|| {
            text.strip_prefix('(')
                .and_then(|value| value.strip_suffix(')'))
        })
        .unwrap_or(text)
        .trim();
    if text.is_empty() || text.contains('\n') {
        return None;
    }
    let path = PathBuf::from(text);
    let path = if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    };
    path.is_file().then_some(path)
}

fn read_git_branch(workspace: &Path) -> Option<String> {
    let head = std::fs::read_to_string(workspace.join(".git/HEAD")).ok()?;
    let head = head.trim();
    head.strip_prefix("ref: refs/heads/")
        .map(str::to_string)
        .or_else(|| (!head.is_empty()).then(|| head.chars().take(7).collect()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileRow {
    pub(super) path: String,
    pub(super) name: String,
    pub(super) depth: usize,
    pub(super) dir: bool,
    pub(super) open: bool,
}

pub(super) fn file_rows(files: &[String], expanded: &[String]) -> Vec<FileRow> {
    let mut rows: Vec<FileRow> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut sorted: Vec<&String> = files.iter().collect();
    sorted.sort();
    for file in sorted {
        let parts: Vec<&str> = file.split('/').filter(|part| !part.is_empty()).collect();
        let mut prefix = String::new();
        let mut hidden = false;
        for (depth, part) in parts.iter().enumerate() {
            let last = depth + 1 == parts.len();
            let path = if prefix.is_empty() {
                (*part).to_string()
            } else {
                format!("{prefix}/{part}")
            };
            if !hidden && seen.insert(path.clone()) {
                let open = last || expanded.contains(&path);
                rows.push(FileRow {
                    path: path.clone(),
                    name: (*part).to_string(),
                    depth,
                    dir: !last,
                    open,
                });
            }
            if !last && !expanded.contains(&path) {
                hidden = true;
            }
            prefix = path;
        }
    }
    rows
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Attach,
    Open,
    Insert,
    Toggle,
    Hunks,
    Diff,
    Merge,
    Discard,
    View,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Context {
    pub(super) col: usize,
    pub(super) row: usize,
    pub(super) cursor: usize,
    pub(super) target: FileRow,
    pub(super) kind: Pane,
}

impl Context {
    pub(super) fn items(&self) -> Vec<(&'static str, Action)> {
        if self.kind == Pane::Worktrees {
            vec![
                ("show diff", Action::Diff),
                ("merge (ff-only)", Action::Merge),
                ("remove worktree (clean only)", Action::Discard),
                ("insert path", Action::Insert),
            ]
        } else if self.kind == Pane::Diff {
            vec![
                ("view hunks", Action::Hunks),
                ("view file", Action::View),
                ("attach @path to prompt", Action::Attach),
                ("open in $EDITOR", Action::Open),
                ("insert path", Action::Insert),
            ]
        } else if self.target.dir {
            vec![
                (
                    if self.target.open {
                        "collapse"
                    } else {
                        "expand"
                    },
                    Action::Toggle,
                ),
                ("insert path", Action::Insert),
            ]
        } else {
            vec![
                ("attach @path to prompt", Action::Attach),
                ("view file", Action::View),
                ("open in $EDITOR", Action::Open),
                ("insert path", Action::Insert),
            ]
        }
    }
}
