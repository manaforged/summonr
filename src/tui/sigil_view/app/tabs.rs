use super::*;
use crate::tui::sigil_view::ui::Tab;

impl App {
    fn tab(&self) -> Option<Tab> {
        match self.overlay() {
            Some(Overlay::Model) => Some(Tab::Models),
            Some(Overlay::Auth)
                if !self.picker.open.peek() && matches!(self.auth.step.peek(), Login::List(_)) =>
            {
                Some(Tab::Providers)
            }
            Some(Overlay::Usage) => Some(Tab::Usage),
            Some(_) | None => None,
        }
    }

    pub(in crate::tui::sigil_view) fn step_tab(&self, delta: isize) -> bool {
        let Some(tab) = self.tab() else {
            return false;
        };
        self.show_tab(tab.step(delta));
        true
    }

    pub(in crate::tui::sigil_view) fn tab_or_pane(&self, forward: bool) {
        if !self.step_tab(if forward { 1 } else { -1 }) {
            self.focus_pane(forward);
        }
    }

    pub(in crate::tui::sigil_view) fn open_usage(&self) {
        self.usage.start();
        drop(self.cmd.send(Cmd::ListUsage));
    }

    pub(in crate::tui::sigil_view) fn show_tab(&self, tab: Tab) {
        let Some(from) = self.tab().filter(|from| *from != tab) else {
            return;
        };
        match from {
            Tab::Models => self.cancel_model_edit(),
            Tab::Providers => self.auth.open.set(false),
            Tab::Usage => self.usage.open.set(false),
        }
        match tab {
            Tab::Models => self.begin_model_edit(),
            Tab::Providers => self.begin_auth(None),
            Tab::Usage => self.open_usage(),
        }
    }
}
