use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sigil::prelude::{Signal, signal};
use sigil_tui::Host;
use tokio::sync::mpsc::UnboundedSender;

use crate::prefs::{self, Prefs};
use crate::provider::UserInput;

use super::super::{Cmd, Info, Msg};
use super::model::{Entry, Model};
use super::suggest::Picker;

pub(super) mod presentation;

use presentation::{facts, header, settings_text, status};

#[derive(Clone)]
pub(super) struct App {
    pub(super) model: Rc<RefCell<Model>>,
    pub(super) cmd: UnboundedSender<Cmd>,
    pub(super) info: Info,
    workspace: PathBuf,
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
    pub(super) task: Signal<String>,
    pub(super) settings: Signal<String>,
    pub(super) facts: Signal<String>,
    pub(super) settings_open: Signal<bool>,
    pub(super) settings_sel: Signal<usize>,
    pub(super) editing_model: Signal<bool>,
    pub(super) model_draft: Signal<String>,
    pub(super) model_caret: Signal<usize>,
    pub(super) model_choices: Signal<Vec<String>>,
    pub(super) model_sel: Signal<usize>,
    pub(super) model_loading: Signal<bool>,
    pub(super) model_error: Signal<String>,
    pub(super) ticks: Signal<u64>,
    pub(super) splash_revealed: Signal<bool>,
    pub(super) splash: Signal<bool>,
    pub(super) theme_name: Signal<String>,
    pub(super) viewport: Signal<(u16, u16)>,
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
            task: signal(String::new()),
            settings: signal(String::new()),
            facts: signal(String::new()),
            settings_open: signal(false),
            settings_sel: signal(0),
            editing_model: signal(false),
            model_draft: signal(String::new()),
            model_caret: signal(0),
            model_choices: signal(Vec::new()),
            model_sel: signal(0),
            model_loading: signal(false),
            model_error: signal(String::new()),
            ticks: signal(0),
            splash_revealed: signal(!info.prefs.animations),
            splash: signal(info.prefs.splash),
            theme_name: signal(info.prefs.theme.clone()),
            viewport: signal((80, 24)),
            quit: signal(false),
            editor_requested: signal(false),
            commands: Picker::new(),
            pasted_images: Rc::new(RefCell::new(Vec::new())),
            git_branch,
            workspace,
            prefs_path: prefs::path(),
            info,
        }
    }

    pub(super) fn update(&self, msg: Msg) {
        match msg {
            Msg::Models(mut models) => {
                let snapshot = self.model.borrow().snapshot.clone();
                let current = format!("{}\t{}", snapshot.endpoint, snapshot.model);
                if !models
                    .iter()
                    .any(|row| row == &current || row == &snapshot.model)
                {
                    models.insert(0, current);
                }
                self.model_choices.set(models);
                self.model_sel.set(0);
                self.model_loading.set(false);
                self.model_error.set(String::new());
            }
            Msg::ModelsFailed(error) => {
                self.model_loading.set(false);
                self.model_error.set(error);
            }
            msg => {
                if !self.model.borrow_mut().apply(msg) {
                    self.quit.set(true);
                }
            }
        }
    }

    pub(super) fn refresh(&self) {
        let old_extent = self.extent.peek();
        let pinned = self.scroll_y.peek() >= old_extent;
        let model = self.model.borrow();
        let transcript = model.transcript();
        let rows = transcript.lines().count().max(1) as f32;
        self.estimated_extent.set(rows * 18.0 + 48.0);
        self.transcript.set(transcript);
        self.header.set(header(&model, &self.info));
        self.status.set(status(&model, &self.info));
        self.task.set(model.task_text());
        self.settings
            .set(settings_text(&model.snapshot, &model.prefs));
        self.facts.set(facts(&model, &self.info));
        self.theme_name.set(model.prefs.theme.clone());
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
        let ticks = self.ticks.peek().saturating_add(1);
        self.ticks.set(ticks);
        let mut model = self.model.borrow_mut();
        if model.busy_since.is_some() {
            model.spinner = (model.spinner + 1) % 10;
        }
        if !self.splash_revealed.peek() && ticks.saturating_mul(40) > 760 {
            self.splash_revealed.set(true);
        }
    }

    pub(super) fn page_history(&self, direction: f32) {
        let page = f32::from(self.viewport.peek().1.saturating_sub(5).max(1)) * Host::CELL;
        let next = self.scroll_y.peek() + direction * page;
        self.scroll_y.set(next.clamp(0.0, self.extent.peek()));
    }

    pub(super) fn scroll_history(&self, delta: f32) {
        let next = self.scroll_y.peek() - delta;
        self.scroll_y.set(next.clamp(0.0, self.extent.peek()));
    }

    pub(super) fn submit(&self) {
        let text = self.draft.peek();
        if text.trim().is_empty() && self.pasted_images.borrow().is_empty() {
            return;
        }
        self.replace(String::new());
        if self.local(&text) {
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

    /// Consume the images pasted since the last submit.
    fn take_pasted(&self) -> Vec<crate::provider::Image> {
        std::mem::take(&mut *self.pasted_images.borrow_mut())
    }

    /// Handle a bracketed paste of one image path. Returns `true` when the
    /// text names a supported image and is consumed as an attachment, so the
    /// caller must not insert the path text verbatim.
    pub(super) fn paste(&self, text: &str) -> bool {
        let Some(path) = image_path(&self.workspace, text) else {
            return false;
        };
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

    /// Record one pasted image, render it in the transcript, and remember it
    /// so the next submit attaches the bytes to the outgoing provider turn.
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
            Some(crate::SlashCommand::Settings) => self.settings_open.set(true),
            Some(crate::SlashCommand::Model) => {
                let model = line.trim_start_matches("/model").trim();
                if model.is_empty() {
                    self.begin_model_edit();
                } else {
                    drop(self.cmd.send(Cmd::SetModel(model.to_string())));
                }
            }
            Some(crate::SlashCommand::Stats) => {
                let rounds = self.model.borrow().usage.clone();
                self.model.borrow_mut().entries.push(Entry::Chart(rounds));
                self.refresh();
            }
            Some(crate::SlashCommand::Theme) => self.toggle_theme(),
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
                .min(10),
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
            5 => self.toggle_theme(),
            6 => {
                let mut model = self.model.borrow_mut();
                model.prefs.splash = !model.prefs.splash;
                let prefs = model.prefs.clone();
                drop(model);
                self.persist(&prefs);
                self.refresh();
            }
            7 => {
                let mut model = self.model.borrow_mut();
                model.prefs.animations = !model.prefs.animations;
                let prefs = model.prefs.clone();
                self.splash_revealed.set(!model.prefs.animations);
                drop(model);
                self.persist(&prefs);
                self.refresh();
            }
            8 => self.toggle_reasoning(),
            9 => self.toggle_tools(),
            _ => self.toggle_bus(),
        }
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
        if let Some((name, model)) = picked.split_once('\t') {
            let current = self.model.borrow().snapshot.endpoint.clone();
            if name != current {
                drop(self.cmd.send(Cmd::SetEndpoint(name.to_string())));
            } else {
                drop(self.cmd.send(Cmd::SetModel(model.to_string())));
            }
        } else {
            drop(self.cmd.send(Cmd::SetModel(picked)));
        }
    }

    pub(super) fn cancel_model_edit(&self) {
        self.editing_model.set(false);
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
        if self.model.borrow().busy_since.is_some() {
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

fn save(path: Option<&Path>, prefs: &Prefs) -> Result<(), String> {
    let path = path.ok_or_else(|| "settings not saved: home directory unavailable".to_string())?;
    prefs::save_in(path, prefs).map_err(|error| format!("settings not saved: {error}"))
}

/// Resolve a bracketed-paste string to an existing image file path.
///
/// Accepts a bare path, or one wrapped in single or double quotes or in
/// parentheses (terminals wrap the pasted clipboard in a different way on
/// some platforms). A relative path resolves against the workspace.
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
