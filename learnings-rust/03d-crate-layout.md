# Crate layout — conventional paths

Cargo uses **conventional paths** to figure out what to build in a crate — there's no "entry" field in `Cargo.toml` for simple cases. It just looks in the right places.

## The conventions

| File | Meaning |
|---|---|
| `src/main.rs` | **Binary crate root.** Executable; must define `fn main()`. |
| `src/lib.rs` | **Library crate root.** A reusable module; no `fn main()`. |
| `src/bin/foo.rs` | **Additional binary**, named `foo`. Run with `cargo run --bin foo`. |
| `src/bin/foo/main.rs` | Same as above when an additional binary needs its own sub-module files. |
| `examples/*.rs` | Example programs, each built as a tiny binary. Run with `cargo run --example foo`. |
| `tests/*.rs` | Integration tests — separate binaries invoked by `cargo test`. |
| `benches/*.rs` | Benchmarks. |

Cargo scans these locations automatically. No `Cargo.toml` entry needed for the common case.

## Bin + lib in the same crate

You can have both `src/main.rs` AND `src/lib.rs` in one crate — common when a binary reuses logic that's also exposed as a library. The binary can `use cawir::something;` to pull from the lib. (Covered in `03a-cargo-cli-and-crate-types.md`.)

## cawir's current split: `main`, `lib`, `repl`, `agent`

After Checkpoint 3.5, cawir has four important top-level files with separate jobs:

| File | Scope |
|---|---|
| `src/main.rs` | Binary entry point. Installs the Tokio runtime with `#[tokio::main]`, calls `cawir::run().await`, and stays tiny. |
| `src/lib.rs` | Library crate root. Declares modules and re-exports the public API, currently `cawir::run`, `cawir::Error`, `cawir::Result`, and `cawir::session`. |
| `src/repl.rs` | Current Surface implementation. Handles `.env`, API key lookup, `reqwest::Client` setup, stdin/stdout, slash commands, and calling the agent for each non-command line. |
| `src/agent.rs` | Core engine orchestration. Runs one user turn: calls the model, executes tools, appends tool results to history, and enforces the tool-loop cap. |

The important Rust distinction:

- `main.rs` exists because Cargo needs a binary target with a `main` function.
- `lib.rs` exists because the reusable application code lives in the library crate named `cawir`.
- `repl.rs` and `agent.rs` are ordinary modules declared by `lib.rs` with `mod repl;` and `mod agent;`.

`lib.rs` re-exports the REPL entry point:

```rust
pub use repl::run;
```

That keeps `main.rs` stable:

```rust
cawir::run().await
```

The design reason is separation by responsibility. `repl.rs` is just one way to talk to the agent. A future TUI, stdio protocol, one-shot CLI command, or WebSocket server should be able to reuse `agent.rs`, `session.rs`, `tools.rs`, and provider code without copying the agent loop.

## Are these paths configurable?

**Yes — but almost nobody changes them.** Each target type accepts a `path` field in `Cargo.toml`:

```toml
[lib]
name = "cawir_core"       # rename the library
path = "core/lib.rs"      # and put it somewhere else

[[bin]]
name = "cawir"
path = "cli/main.rs"

[[example]]
name = "demo"
path = "playground/demo.rs"

[[test]]
name = "integration"
path = "it/main.rs"

[[bench]]
name = "tokenize"
path = "perf/tokenize.rs"
```

Everything is movable. `path` is relative to `Cargo.toml`.

## Disabling auto-discovery

If you want to declare targets *only* explicitly (no scanning), `[package]` has flags:

```toml
[package]
autobins = false       # don't scan src/bin/
autoexamples = false   # don't scan examples/
autotests = false      # don't scan tests/
autobenches = false    # don't scan benches/
```

Each defaults to `true`. Useful for weird layouts or when you want Cargo to treat some files as plain source, not as targets.

## Should you actually change these?

**Almost never.** The reason:

- **Ecosystem uniformity.** Open any crate on GitHub — you know `src/main.rs` is the entry point, tests are in `tests/`, examples in `examples/`. That uniformity is a genuine quality-of-life feature.
- **Tooling.** rust-analyzer, `cargo-*` subcommands, docs generators all assume the conventions. They'll still work with custom paths, but with more friction.
- **New contributors.** A custom layout adds a "what's going on here?" tax to anyone opening the repo.

Reasons people *do* change them:

1. Migrating a non-Rust codebase where files are already laid out differently and moving them is painful.
2. Unusual multi-target crates where the conventional layout gets cluttered.
3. Sharing code between several binaries via a common folder structure.

For cawir: we stick with defaults entirely.

## `src/` isn't special — the entry files are

Note: `src/` as a directory name isn't technically required. What Cargo actually looks for is `src/main.rs`, `src/lib.rs`, etc. at specific paths. You can override each of those paths individually. But again, don't.
