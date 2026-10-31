//! MCP (Model Context Protocol) client for rig: servers over stdio, with their tools loaded on
//! demand. See `tools` for what the model sees.

mod client;
mod hub;
mod tools;

pub use client::RemoteTool;
pub use hub::{Hub, ServerSpec};
pub use tools::{catalog, tools};

#[cfg(test)]
mod tests;
