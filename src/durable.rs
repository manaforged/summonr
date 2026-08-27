//! Durable sessions.
//!
//! `SessionDriver` is the only writer. A session is two files under
//! `<home>/sessions/<id>/`: `session.jsonl`, the conversation, and
//! `work.json`, the job. Resume reads both; git is read for the changed-file
//! manifest and never written.
pub(crate) mod driver;
pub(crate) mod id;
pub(crate) mod log;
pub(crate) mod tree;
pub(crate) mod work;

#[cfg(test)]
#[path = "durable_tests.rs"]
mod tests;
