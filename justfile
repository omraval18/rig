# Common tasks. `just` with no arguments lists them.
default:
    @just --list

fmt:
    cargo fmt --all

clippy:
    cargo clippy --workspace --all-targets

test:
    cargo test --workspace

# What to run before committing.
check: fmt clippy test

release:
    cargo build --release
