//! Sigil presentation for Summoner's asynchronous agent worker.

mod app;
mod canvas;
mod chat;
mod host;
mod model;
mod scene;
mod settings;
mod suggest;
mod view;

pub(super) use host::run;

#[cfg(test)]
#[path = "sigil_view_tests.rs"]
mod tests;
