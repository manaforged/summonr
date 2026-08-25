//! Format-2 durable execution primitives.
//!
//! `SessionDriver` is the format-2 owner. `Loop` writes format-2. The reducer
//! is the live and resume program. Work projections store step and goal
//! progress. The driver does not keep a second copy of session state.
#![allow(
    dead_code,
    reason = "Work and effect facts compile for the reducer and later slices"
)]

pub(crate) mod bound;
pub(crate) mod context;
pub(crate) mod driver;
pub(crate) mod effect;
pub(crate) mod encoding;
pub(crate) mod fact;
pub(crate) mod id;
pub(crate) mod journal;
pub(crate) mod observe;
pub(crate) mod product;
pub(crate) mod project;
pub(crate) mod publish;
pub(crate) mod reducer;
pub(crate) mod work;
pub(crate) mod workspace;

#[cfg(test)]
#[path = "durable_tests.rs"]
mod tests;
