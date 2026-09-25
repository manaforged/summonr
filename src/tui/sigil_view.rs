mod app;
mod canvas;
pub(in crate::tui) mod chat;
mod demo;
#[cfg(feature = "gui")]
mod desktop;
#[cfg(feature = "gui")]
mod gui;
mod help;
mod host;

mod model;
pub(in crate::tui) mod overlay;
mod pane;
mod panes;
pub(in crate::tui) mod prelude;
pub(super) mod profile;
#[cfg(test)]
mod scale_tests;
mod scene;
mod search;
mod sessions;
pub(in crate::tui) mod settings;
mod speed;
mod suggest;
mod theme;
mod usage;
mod view;

pub(crate) use canvas::ellipsize;
#[cfg(feature = "gui")]
pub(super) use gui::run as run_gui;
pub(super) use host::run::run;
