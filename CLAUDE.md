# cawir — Coding Agent Written in Rust

## What this project is

A minimal coding agent, written in Rust, built incrementally as a learning vehicle. Two goals held equally:

1. **Learn Rust** — the author is new to Rust. Every step should teach a Rust concept, not just produce working code.
2. **Learn how coding agents work** — by building one from scratch (model loop, tool use, permissions, context), not just using one.

The target is *not* to compete with Claude Code or Cursor. The target is a small, readable agent the author fully understands.

## How to collaborate on this project

### Teach as you go

The author does not know Rust. When you write or review code here:

- **Explain new Rust concepts inline** the first time they appear — ownership, borrowing, lifetimes (`'a`), traits, `impl`, `Result<T, E>`, `Option<T>`, `?`, `async`/`await`, `match`, pattern binding, modules, `Cargo.toml`, crates, `derive` macros, error types, `Box`/`Arc`/`Rc`, `&str` vs `String`.
- **Contrast with other languages** briefly when it helps. The author has general programming experience; mapping Rust concepts to their analogues elsewhere is useful.
- **Prefer the idiomatic Rust way** and say *why* it's idiomatic — don't just translate from another language.
- **One idea per step.** If a change introduces a new language feature *and* a new architectural concept, split them.

### Grow the codebase organically

Do not scaffold a large architecture upfront. Each addition should be motivated by a concrete need the author can see. Rough progression the project is aimed at (subject to change — don't implement ahead):

1. `cargo new` + hello world — understand `Cargo.toml`, `src/main.rs`, `cargo run`, `cargo build`.
2. Read a prompt from stdin, print it back. Introduce `String`, `io`, `Result`, `?`.
3. Call the Anthropic Messages API with that prompt, print the response. Introduce crates (`reqwest`, `serde`, `tokio`), async, JSON (de)serialization, env vars for the API key.
4. Turn it into a loop — multi-turn conversation. Introduce `Vec`, struct definitions for messages.
5. Add a single read-only tool (e.g. `read_file`). Introduce tool-use schema, `match` on tool names, error propagation.
6. Add a write tool with a permission prompt. Introduce user-confirmation flow.
7. Further tools, streaming, context management, etc. — decide later.

### Code style

- No unused features, no speculative abstractions. Three similar lines beats a premature trait.
- Comments only when the *why* is non-obvious. Rust's type system already documents most *what*s.
- When introducing a new crate, explain what it does and why this crate vs alternatives (briefly).

## Current state

Toolchain installed and verified (2026-04-23). No project code yet. Two foundational decisions still pending before `cargo new`.

## Pending decisions (to discuss in depth, in order)

These are not yet decided. Do not assume defaults — work through them with the author before writing code or updating this file with the outcome.

1. **Model provider & API shape** — Anthropic Messages API vs OpenAI vs local (Ollama) vs provider-agnostic abstraction; tradeoffs for a *learning* project (clarity of the wire format, quality of tool-use support, cost, offline-ability); whether to hand-roll HTTP+JSON or use an SDK; how the API key is supplied.

Once decided, move it to the "Conventions" section with the outcome and rationale.

## Conventions

### Toolchain (decided 2026-04-23)

- **Installer:** `rustup` (official), installed via `curl https://sh.rustup.rs | sh`. Not Homebrew.
- **Channel:** `stable`. No `rust-toolchain.toml` pin yet — add one only when we hit a reason.
- **Profile:** `default` — ships `rustc`, `cargo`, `rust-std`, `rust-docs`, `rustfmt`, `clippy`. Use all four tools from day one.
- **Current version:** Rust 1.95.0 stable (as of install). `rustup update` refreshes it.
- **PATH:** `~/.cargo/bin` added via `~/.zshenv` (not `~/.zshrc`) so non-interactive shells/cron/launchd/subprocesses also see cargo.
- **Linter policy:** run `cargo clippy` regularly while learning — its suggestions are one of the best free Rust tutors. `cargo fmt` before committing.

### Editor / IDE (decided 2026-04-23)

- **Editor:** VS Code.
- **LSP:** `rust-analyzer` VS Code extension (publisher: *The Rust Programming Language*, id `rust-lang.rust-analyzer`). Extension auto-downloads the rust-analyzer binary on first `.rs` file open. Do **not** install the deprecated `rust-lang.rust` (RLS-based) extension.
- **Debugger:** `CodeLLDB` extension (id `vadimcn.vscode-lldb`). Wraps LLDB via DAP. Understands Rust pretty-printers (`Vec`, `Option`, `Result`).
- **Editor config in repo:** not committed. Solo learning project — keep the repo clean. Revisit if collaboration starts.
- **Verified 2026-04-23:** rust-analyzer binary downloaded (v0.3.2870-standalone), inlay hints render, hover docs work, error diagnostics + quick-fix suggestions work. CodeLLDB installed but not yet exercised — will be tested when cawir has code worth stepping through.

### Still to be decided

- _Model provider & API shape_: pending discussion.
- _Project layout_: TBD — will follow Cargo defaults until there's a reason not to.
- _Error handling_: TBD — likely start with `Box<dyn Error>`, graduate to `anyhow`/`thiserror` when the tradeoffs are visible.
- _Async runtime_: TBD — `tokio` is the default when async is introduced.

Update this file as real conventions get chosen — don't document guesses.
