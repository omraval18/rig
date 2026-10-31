# Working on rig

The conventions follow [openai/codex](https://github.com/openai/codex)'s Rust workspace so the code reads the same way.

## Layout

- `rig-core` has no network and no terminal. `rig-tools` and `rig-provider` depend on it, `rig-cli` on all three. Keep the arrows pointing one way.
- One responsibility per file. Keep modules under about 500 lines (hard limit 800); when one grows past that, split it into a directory with `mod.rs` and focused children. `Tui` is a good model: state in `state.rs`, one file each for keys, events, drawing, submit.
- Tests live next to the code: `foo.rs` ends with `#[cfg(test)] mod tests;` and the tests are in `foo/tests.rs` (`mod.rs` files use a sibling `tests.rs`). Tests of a few lines may stay inline.

## Rust style

- `cargo fmt` (edition 2024) and `cargo clippy --workspace --all-targets` must be clean. Lints are set once in the root `Cargo.toml` (`[workspace.lints.clippy]`); a new crate opts in with `[lints] workspace = true`.
- No `unwrap()` or `expect()` outside tests. Lock a mutex with `rig_core::sync::MutexExt::locked` (and `RwLockExt`); return errors with `?` or turn them into a tool result.
- Inline format arguments: `format!("{name}: {n}")`, not `format!("{}: {}", name, n)`.
- Prefer a builder or a small constructor over a struct literal repeated in many places (`PickItem::new(..).detail(..)`, `Completion::assistant(..)`, `Endpoint::new(..)`).
- Share a helper once it is needed twice: `rig_core::text` and `rig_core::fsutil` for text and filesystem, `style.rs` for every colour. Do not define a second palette.
- State that belongs together lives in one struct (`Prompt`, `Turn`, `Display`) instead of dozens of fields on `Tui`.
- Allow a lint only with a comment saying why (`#[allow(clippy::needless_collect)]` in `agent/calls.rs`).

## Changing behavior

- The UI is covered by pty scenarios run against a real terminal emulator; unit tests cover the rest. Run `just check` before committing, and try the TUI when you change anything under `rig-cli/src/{tui,view,screen}`.
- Do not add a `Co-Authored-By` line (or any trailer) to commit messages.
