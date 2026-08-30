mod app;
mod canvas;
mod chat;
#[cfg(feature = "gui")]
mod desktop;
mod editor;
#[cfg(feature = "gui")]
mod gui;
mod hit;
mod host;
mod model;
mod pane;
mod panes;
mod patch;
mod scene;
mod settings;
mod suggest;
mod view;

#[cfg(feature = "gui")]
pub(super) use gui::run as run_gui;
pub(super) use host::run;

#[cfg(test)]
#[path = "sigil_view_tests.rs"]
mod tests;
