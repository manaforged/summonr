use super::*;
use crate::prefs::{Background, THEMES};
use crate::tui::sigil_view::theme;
use crate::tui::sigil_view::ui::cycle;
use sigil::widgets::CursorEdge;

impl App {
    pub(in crate::tui::sigil_view) fn open_theme(&self) {
        let editor = self.theme_editor;
        let model = self.model.peek();
        let (palette, problems) =
            theme::load(&model.prefs, &self.workspace(), self.info.config.as_deref());
        editor.before.set(self.appearance.peek());
        editor.palette.set(palette);
        editor.name.set(model.prefs.theme.clone());
        editor.error.set(problems.join("\n"));
        editor.editing.set(None);
        editor.list.select(0);
        editor.list.scroll.set(0.0);
        self.settings_open.set(false);
        editor.open.set(true);
    }

    fn preview_theme(&self, mut palette: Palette) {
        if self.model.peek().prefs.background == Background::Terminal {
            palette.base.a = 0;
        }
        theme::set(palette);
        self.appearance.set(palette);
        self.model_mut(|model| model.entries.invalidate());
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn preview_color(&self, value: &str) {
        let editor = self.theme_editor;
        let Some((name, _)) = editor.editing.peek() else {
            return;
        };
        editor.input.set(value.to_string());
        match theme::hex(value) {
            Ok(color) => {
                editor.palette.update(|palette| {
                    palette.set(name, color);
                });
                editor.error.set(String::new());
                self.preview_theme(editor.palette.peek());
            }
            Err(error) => editor.error.set(error.to_string()),
        }
    }

    pub(in crate::tui::sigil_view) fn edit_theme(&self) {
        let editor = self.theme_editor;
        if editor.editing.peek().is_some() {
            self.preview_color(&editor.input.peek());
            if editor.error.peek().is_empty() {
                editor.editing.set(None);
            }
            return;
        }
        let at = editor.list.cursor.peek();
        if at == 0 {
            self.step_theme(1);
            return;
        }
        if let Some((name, color)) = editor.palette.peek().colors().get(at - 1).copied() {
            let value = color_hex(color);
            editor.caret.set(value.len());
            editor.input.set(value);
            editor.error.set(String::new());
            editor.editing.set(Some((name, color)));
            editor.focus.set(editor.focus.peek() + 1);
        }
    }

    pub(in crate::tui::sigil_view) fn move_theme(&self, delta: isize) {
        let editor = self.theme_editor;
        if editor.editing.peek().is_none() {
            editor.list.shift(
                delta,
                editor.palette.peek().colors().len() + 1,
                CursorEdge::Clamp,
            );
        }
    }

    pub(in crate::tui::sigil_view) fn step_theme(&self, delta: isize) {
        let editor = self.theme_editor;
        if editor.editing.peek().is_some() {
            return;
        }
        let name = editor.name.peek();
        let at = THEMES
            .iter()
            .position(|preset| *preset == name)
            .unwrap_or(0);
        let next = cycle(at, delta, THEMES.len());
        editor.name.set(THEMES[next].to_string());
        self.reset_theme();
    }

    pub(in crate::tui::sigil_view) fn reset_theme(&self) {
        let editor = self.theme_editor;
        editor.editing.set(None);
        editor.error.set(String::new());
        editor.palette.set(Palette::named(&editor.name.peek()));
        self.preview_theme(editor.palette.peek());
    }

    pub(in crate::tui::sigil_view) fn cancel_theme(&self) {
        let editor = self.theme_editor;
        if let Some((name, before)) = editor.editing.peek() {
            editor.palette.update(|palette| {
                palette.set(name, before);
            });
            editor.editing.set(None);
            editor.error.set(String::new());
            self.preview_theme(editor.palette.peek());
        } else {
            self.close_theme();
        }
    }

    pub(in crate::tui::sigil_view) fn close_theme(&self) {
        let editor = self.theme_editor;
        editor.open.set(false);
        editor.editing.set(None);
        self.preview_theme(editor.before.peek());
        self.settings_open.set(true);
    }

    pub(in crate::tui::sigil_view) fn select_theme(&self, name: &str) {
        self.model_mut(|model| model.prefs.theme = crate::prefs::theme(name).to_string());
        self.persist(&self.model.peek().prefs);
        self.theme_editor.open.set(false);
        self.theme_editor.editing.set(None);
        self.settings_open.set(true);
        self.apply_theme();
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn save_theme(&self) {
        let editor = self.theme_editor;
        if editor.editing.peek().is_some() {
            self.edit_theme();
            if !editor.error.peek().is_empty() {
                return;
            }
        }
        let path = self.workspace().join(".summonr").join("theme.toml");
        match editor.palette.peek().save(&path) {
            Ok(()) => self.select_theme(&editor.name.peek()),
            Err(error) => editor.error.set(format!("Could not save colors: {error}")),
        }
    }
}
