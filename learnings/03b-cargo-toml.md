# `Cargo.toml` — the manifest

```toml
[package]
name = "cawir"
version = "0.1.0"
edition = "2024"

[dependencies]
```

## What TOML is

**TOML** = **T**om's **O**bvious **M**inimal **L**anguage. Named after its creator, **Tom** Preston-Werner (co-founder of GitHub). The name is tongue-in-cheek — the idea being that config files should be, well, obvious and minimal.

Released in 2013, designed specifically for **configuration files** (not data interchange — it's not competing with JSON for APIs).

### Why it exists

The config-file landscape before TOML:

| Format | Problem |
|---|---|
| **JSON** | No comments. Picky about trailing commas. All strings must be quoted. Humans hate writing it by hand. |
| **YAML** | Too flexible — indentation-sensitive and has genuine ambiguities (e.g. `country: no` becomes the boolean `false` in some YAML parsers — the famous "Norway problem"). |
| **INI** | Simple, but no standard spec — every parser handles nesting, types, and escaping differently. |
| **XML** | Verbose to the point of being unpleasant. |

TOML's pitch: **unambiguously parseable, comment-friendly, tolerant of human editing, no silent coercion surprises.** Every type is explicit — `"1.0"` is a string, `1.0` is a float, `true` is a bool, `1979-05-27` is a date.

### Where else you'll see TOML

- **Rust/Cargo** — `Cargo.toml`, `rust-toolchain.toml`
- **Python** — `pyproject.toml` (replaces the old `setup.py` for modern Python packaging)
- **Hugo** (static site generator), **Poetry**, **Black**, **uv**, many CLI tools

### Syntax basics

```toml
[section]               # introduces a table
key = "value"           # string
number = 42             # integer
pi = 3.14               # float
ok = true               # boolean
items = ["a", "b", "c"] # array
date = 1979-05-27       # date (first-class type)

[nested.section]        # dotted keys for nesting
foo = "bar"

[[array_of_tables]]     # double-bracket = array of tables
name = "first"

[[array_of_tables]]
name = "second"
```

The last form — double-bracket `[[...]]` — matters because Cargo uses it for `[[bin]]`, `[[example]]`, `[[test]]`, `[[bench]]` (see `03d-crate-layout.md`).

## `[package]` section — metadata

| Field | What it means |
|---|---|
| `name` | The crate's name. Becomes the default binary name, the identifier for `cargo add`, and the name used if you publish to crates.io. Must be unique on crates.io if you publish. Conventionally `snake_case` or `kebab-case`. |
| `version` | [SemVer](https://semver.org). Rust takes SemVer seriously — breaking changes to a library require a major bump; Cargo's resolver relies on this for library compatibility. For a binary it matters less but is conventional. |
| `edition` | The Rust edition this crate opts into. See `04-rust-editions.md`. |

Many more `[package]` fields exist (publication-oriented for crates.io):

- `authors` — list of author strings (deprecated-ish; usually omitted now)
- `description` — one-sentence summary shown on crates.io
- `license` — SPDX identifier (`"MIT"`, `"Apache-2.0 OR MIT"`, etc.)
- `repository` — URL to source repo
- `homepage` — URL to project homepage
- `readme` — path to README (defaults to `README.md`)
- `keywords` — tags for discoverability on crates.io
- `categories` — crates.io category list
- `rust-version` — MSRV (Minimum Supported Rust Version), e.g. `"1.75"`
- `default-run` — which binary `cargo run` picks when multiple exist

Cargo doesn't require any of these unless you publish.

## `[dependencies]` — runtime deps

Empty right now. Each `cargo add <crate>` appends a line:

```toml
[dependencies]
reqwest = "0.12"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

Two syntaxes for a dep:

- **Simple:** `name = "version"` — pulls from crates.io with default features.
- **Detailed (inline table):** `name = { version = "...", features = [...], default-features = false, optional = true, path = "...", git = "...", branch = "...", rev = "..." }`.

The detailed form lets you:

- Enable specific **features** (compile-time flags the crate exposes)
- Disable default features
- Pull from a local path instead of crates.io (for sibling crates)
- Pull from a git repo (for forks or unreleased versions)

## Full catalog of other sections

### Tier 1 — you'll definitely use these

| Section | Purpose |
|---|---|
| `[package]` | Metadata (above). |
| `[dependencies]` | Runtime deps — compiled into your binary/library. |
| `[dev-dependencies]` | Deps used only in tests, examples, and benchmarks. Excluded from release builds. |
| `[profile.dev]` / `[profile.release]` | Compiler flags per build mode — opt level, debug info, LTO, panic behavior, codegen units. |

### Tier 2 — common, situational

| Section | Purpose |
|---|---|
| `[features]` | Optional compile-time flags. `cargo build --features foo` pulls in extra deps or code paths. Huge topic — how Rust does "opt-in" functionality. |
| `[build-dependencies]` | Deps used by `build.rs`, a script that runs *at compile time* (for code generation, linking C libs, etc.). Not compiled into your crate. |
| `[lib]` | Configures the library target — its name, which `crate-type`s to emit (`rlib`, `cdylib`, `staticlib`, `proc-macro` — see `03a-cargo-cli-and-crate-types.md`), whether to include `doctest`s. |
| `[[bin]]` | **Array of tables** (note the double brackets). Lets you declare extra binaries or override the default one's settings. Each `[[bin]]` block is one binary. |
| `[[example]]`, `[[test]]`, `[[bench]]` | Same shape — extra examples / tests / benchmarks with custom config. (See `03d-crate-layout.md`.) |
| `[lints]` | Crate-level lint configuration — `rust` lints, `clippy` lints, `rustdoc` lints, each with `allow` / `warn` / `deny` levels. Nicer than scattering `#![warn(...)]` attributes in source files. |
| `[workspace]` | Declares a **workspace** — a multi-crate repo where several Cargo.toml files share one `Cargo.lock` and one `target/`. Used by big projects (Tokio, serde, rust-analyzer itself). |

### Tier 3 — specialized / rare

| Section | Purpose |
|---|---|
| `[target.'cfg(...)'.dependencies]` | Platform-conditional deps. Example: `[target.'cfg(windows)'.dependencies] winapi = "..."` — only depended on when compiling for Windows. The `cfg(...)` syntax is full-featured. |
| `[patch]` / `[patch.crates-io]` | Override a dep's source. Example: temporarily use your GitHub fork of `reqwest` while a fix is in review. |
| `[package.metadata.<tool>]` | Arbitrary config for third-party tools that want to piggyback on Cargo.toml. Example: `[package.metadata.docs.rs]` for docs.rs build flags. Cargo itself ignores this — it's a namespaced scratchpad. |
| `[workspace.dependencies]`, `[workspace.package]`, `[workspace.lints]` | Workspace-level defaults. Members inherit via `version.workspace = true`. |
| `[badges]` | Near-obsolete. Used to render status badges on crates.io. Most people use README shields now. |
| `[replace]` | Older dep-override mechanism. Superseded by `[patch]`. Don't use. |

## Forecast: what cawir's Cargo.toml grows into

Rough projection based on the plan in CLAUDE.md:

| Stage | New sections / deps |
|---|---|
| v0 (Anthropic only) | `[dependencies]` gains `reqwest`, `tokio`, `serde`, `serde_json` |
| v1 (add OpenAI, extract Provider trait) | same — no new sections |
| v2 (add Ollama, tests) | `[dev-dependencies]` gains an HTTP mocking crate |
| Later (maybe) | `[features]` if we want optional provider flags |
| Later (maybe) | `[lints]` once we've agreed on a lint policy |
| Later (maybe) | Split into `[lib]` + `[[bin]]` once the agent logic is big enough to want as a library |

Unlikely to ever need: `[workspace]`, `[patch]`, `[build-dependencies]`, `[target.<cfg>.*]`, `[features]` for platform splits. cawir is a self-contained binary — no FFI, no platform splits, no code generation.
