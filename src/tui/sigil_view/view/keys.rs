use super::*;

use crate::prefs::Hotkey;
use crate::runtime::GrantAnswer;
use crate::tui::sigil_view::app::Reply;

mod bindings;
mod nav;

pub(in crate::tui::sigil_view) use bindings::KeyCommand;
use bindings::bindings;

impl App {
    pub(in crate::tui::sigil_view) fn run_key(&self, command: KeyCommand) -> bool {
        match command {
            KeyCommand::Hotkey(hotkey) => match hotkey {
                Hotkey::Options => self.toggle_options(),
                Hotkey::Newline => return false,
                Hotkey::Editor => self.editor_requested.set(true),
                Hotkey::Files => self.toggle_pane(Pane::Files),
                Hotkey::Ledger => self.toggle_todos(),
                Hotkey::Agents => self.toggle_agents(),
                Hotkey::Diff => self.toggle_pane(Pane::Diff),
                Hotkey::Copy => self.copy_chat(None),
                Hotkey::Select => self.select.set(!self.select.peek()),
                Hotkey::Pane => self.toggle_side(),
                Hotkey::Search => self.search.start(""),
                Hotkey::History => self.prompt_history(""),
                Hotkey::Fold => self.fold_cycle(),
            },
            KeyCommand::Trigger(trigger) => self.open_trigger(trigger),
            KeyCommand::NewSession => self.open_session(crate::tui::hub::Open::New),
            KeyCommand::Pane(pane) => self.toggle_pane(pane),
            KeyCommand::Always => self.approval_answer(GrantAnswer::Always),
            KeyCommand::Retry => self.retry(),
            KeyCommand::Interrupt => self.interrupt(),
            KeyCommand::Quit => self.quit.set(true),
            KeyCommand::ToggleRecent => self.toggle_recent(),
            KeyCommand::CopyPatch => self.copy_patch(),
            KeyCommand::ViewExternal => self.view_external(),
            KeyCommand::Collapse => self.pane_collapse(),
            KeyCommand::Expand => self.pane_expand(),
            KeyCommand::TabOrPane(forward) => self.tab_or_pane(forward),
            KeyCommand::NudgeSplit(delta) => self.nudge_split(delta),
            KeyCommand::HunkAccept => self.hunk_accept(),
            KeyCommand::HunkReject => self.hunk_reject(),
            KeyCommand::Shortcuts => return self.shortcuts(),
            KeyCommand::Answer(reply, answer) => return self.answer(reply, answer),
            KeyCommand::Escape => self.escape(),
            KeyCommand::Enter => return self.enter(),
            KeyCommand::Edit => return self.edit_key(),
            KeyCommand::Save => return self.save_key(),
            KeyCommand::Note => return self.note_key(),
            KeyCommand::Edge(end) => return self.edge(end),
            KeyCommand::CtrlEdge(end) => return self.ctrl_edge(end),
            KeyCommand::Page(dir) => return self.page(dir),
            KeyCommand::Arrow(dir) => return self.arrow(dir),
            KeyCommand::Adjust(dir) => return self.overlay_adjust(dir),
            KeyCommand::Toggle => return !self.options_active() && self.form_toggle(),
            KeyCommand::Recall(older) => return self.recall(older),
        }
        true
    }

    fn escape(&self) {
        if self.hotkeys.armed.peek() {
            self.hotkeys.armed.set(false);
            return;
        }
        if self.options_menu.peek() {
            self.options_menu.set(false);
            return;
        }
        if self.options_active() {
            super::input::escape(self);
        } else if self.dismiss() || self.edit_stop() || self.note_stop() {
        } else if self.pane_focus.peek() {
            self.focus_pane(false);
        } else if self.pane_back() || self.pane_dismiss() {
        } else if self.form_stage().is_some() {
            self.form_reply(Reply::Cancel);
        } else {
            super::input::escape(self);
        }
    }

    fn enter(&self) -> bool {
        if self.note_target.peek().is_some() && !self.options_active() {
            self.note_commit(&self.note_text.peek());
            return true;
        }
        if self.view_edit.peek() && !self.options_active() {
            return false;
        }
        if self.overlay_enter() {
        } else if self.pane_active().is_some() {
            self.pane_enter();
        } else if self.form_enter() {
        } else if !self.accept() {
            self.submit();
        }
        true
    }

    fn review(&self) -> bool {
        !self.options_active()
            && self.form_stage() == Some(crate::tui::sigil_view::app::Stage::Review)
    }

    fn answering(&self) -> bool {
        !self.options_active() && self.draft.peek().is_empty() && self.approval.peek().is_some()
    }

    fn answer(&self, reply: Reply, answer: GrantAnswer) -> bool {
        if self.review() {
            self.form_reply(reply);
        } else if self.answering() {
            self.approval_answer(answer);
        } else {
            return false;
        }
        true
    }

    fn edit_key(&self) -> bool {
        if self.review() {
            self.form_reply(Reply::Edit);
        } else if self.pane_active() == Some(Pane::View) {
            self.edit_start();
        } else {
            return false;
        }
        true
    }

    fn save_key(&self) -> bool {
        if self.picker.open.peek() {
            return true;
        }
        let save = self.view_edit.peek() && !self.hotkeys.recording.peek();
        if save {
            self.edit_save();
        }
        save
    }

    fn note_key(&self) -> bool {
        if self.pane_active() != Some(Pane::Diff) {
            return false;
        }
        let Some((path, hunk)) = self.current_hunk() else {
            return false;
        };
        self.note_start(path, hunk);
        true
    }

    fn shortcuts(&self) -> bool {
        let shown = !self.options_active()
            && !self.view_edit.peek()
            && !self.pane_focus.peek()
            && self.form_stage().is_none()
            && self.approval.peek().is_none()
            && self.draft.peek().is_empty();
        if shown {
            self.help.show();
            self.refresh();
        }
        shown
    }
}

pub(in crate::tui::sigil_view) fn shell(app: &App) -> NodeBuilder {
    let mut shell = super::input::leader_shortcuts(
        container()
            .role(Role::Main)
            .label("Summonr agent harness")
            .layout(LayoutStyle::flex_column().fill_width().fill_height()),
        app,
    );
    for (shortcut, when, command) in bindings() {
        let app = app.clone();
        let handler = move |event: &mut KeyEvent| {
            if when.holds(&app) && app.run_key(command) {
                event.stop_propagation();
            }
        };
        shell = if matches!(command, KeyCommand::Recall(_)) {
            shell.key(shortcut, handler)
        } else {
            shell.key_capture(shortcut, handler)
        };
    }
    let scroll = app.clone();
    let owner = app.clone();
    let stray = app.clone();
    let shell = shell
        .key_any(move |event| stray.type_elsewhere(event))
        .key_capture_any(move |event| {
            if super::input::leader_plain(&owner, event) {
                event.stop_propagation();
                return;
            }
            let typed = event
                .text
                .as_ref()
                .is_some_and(|text| !text.is_empty() && !text.chars().any(char::is_control))
                && !crate::tui::sigil_view::host::intake::chord(event.modifiers);
            owner.settle.set(false);
            if typed && owner.pane_active().is_some() {
                event.stop_propagation();
            }
        })
        .on_scroll(move |event| scroll.wheel(event));
    let shell = super::input::tab_shortcuts(shell, app);
    super::input::hotkey_shortcuts(super::input::composer_keys(shell, app), app)
}

impl App {
    fn type_elsewhere(&self, event: &mut KeyEvent) {
        let Some(text) = event.text.as_ref().map(ToString::to_string) else {
            return;
        };
        let printable = !text.is_empty() && !text.chars().any(char::is_control);
        let plain = !crate::tui::sigil_view::host::intake::chord(event.modifiers);
        let free = self.overlay().is_none()
            && self.pane_active().is_none()
            && !self.view_edit.peek()
            && !self.hotkeys.recording.peek();
        if super::canvas::is_window() && printable && plain && free {
            event.stop_propagation();
            self.type_into_composer(&text);
        }
    }

    fn wheel(&self, event: &mut sigil::events::ScrollEvent) {
        self.settle.set(false);
        let lines = super::canvas::scroll_lines(event.delta);
        let step = -lines.round() as isize;
        if self.wheel_surface(step, event.position.x) {
            event.stop_propagation();
            return;
        }
        let before = self.scroll_y.peek();
        self.scroll_history(lines * super::canvas::cell().h);
        if self.scroll_y.peek() != before {
            event.stop_propagation();
        }
    }

    fn wheel_surface(&self, step: isize, x: f32) -> bool {
        if super::canvas::is_window() {
            return self.options_active() && self.overlay_scroll(step);
        }
        if self.overlay_move(step) {
            return true;
        }
        let over_pane = !self.options_active()
            && !self.splash.peek()
            && self.layout.peek().secondary.is_some()
            && x >= f32::from(self.chat_cols()) * super::canvas::cell().w;
        if over_pane {
            self.pane_move(step);
        }
        over_pane
    }
}
