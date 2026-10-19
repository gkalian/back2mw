//! Оркестрация: конфигурация, стадии `extract` и `validate`, прогон `all` для среза M1.
//!
//! Правила границ (`docs/ru/architecture.md`): этот крейт — единственный, кто видит все остальные.

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
