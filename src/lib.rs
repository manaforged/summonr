#![recursion_limit = "256"]

#[path = "main.rs"]
mod entry;
#[cfg(feature = "gui")]
pub use entry::desktop;
pub use entry::main;
pub mod update;
