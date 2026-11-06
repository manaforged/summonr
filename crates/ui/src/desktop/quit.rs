use std::cell::{Cell, RefCell};

use sigil::app::CloseAction;
use summonr_agent::hub::Status;

use super::App;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static LEAVING: Cell<bool> = const { Cell::new(false) };
}

/// Lets the close hook see this window's agents.
pub(super) fn install(app: &App) {
    APP.with(|slot| *slot.borrow_mut() = Some(app.clone()));
}

/// Closes the window, or asks first while an agent is running.
pub(super) fn on_close() -> CloseAction {
    if LEAVING.with(Cell::get) {
        return CloseAction::Close;
    }
    let running = APP.with(|slot| {
        slot.borrow().as_ref().map_or(0, |app| {
            app.model.with(|model| {
                model
                    .live
                    .iter()
                    .filter(|entry| matches!(entry.status, Status::Working | Status::NeedsYou))
                    .count()
            })
        })
    });
    if running == 0 {
        return CloseAction::Close;
    }
    super::agent_menu::ask_quit(running);
    CloseAction::Keep
}

/// Quits the app by the same rule as closing the window.
pub(super) fn request() {
    if on_close() == CloseAction::Close {
        sigil::request_exit();
    }
}

/// Quits after the person confirmed it.
pub(super) fn leave() {
    LEAVING.with(|leaving| leaving.set(true));
    sigil::request_exit();
}
