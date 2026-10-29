//! Orchestration: configuration, the `extract` and `validate` stages, the `all` run for the M1
//! slice.
//!
//! Boundary rules (`docs/en/architecture.md`): this crate is the only one that sees all the others.

pub(crate) mod bodies;
pub mod config;
pub(crate) mod creature_models;
pub mod detect;
pub(crate) mod dialogue;
pub mod extract;
pub(crate) mod faces;
pub mod install;
pub mod job;
pub mod package;
pub mod pipeline;
pub mod plugins;
pub mod settings;
pub mod validate;
pub mod world_map;
pub mod worlds;
