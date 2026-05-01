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
