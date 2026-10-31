//! rig-core: the provider-independent harness.
//!
//! This crate has no network or tool implementations. It defines the
//! conversation model, the [`Provider`](provider::Provider) and
//! [`Tool`](tool::Tool) seams, the permission policy, and the agent loop.

pub mod agent;
pub mod commands;
pub mod compact;
pub mod expand;
pub mod fsutil;
pub mod fuzzy;
pub mod image;
pub mod message;
pub mod permission;
pub mod pricing;
pub mod prompt;
pub mod provider;
pub mod session;
pub mod skills;
pub mod sync;
pub mod text;
pub mod tool;
