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

## Pending decisions

All three foundational decisions now settled. See Conventions below.

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

### Model provider & API shape (decided 2026-04-23)

**Product framing.** cawir is a BYO-model coding agent. The user supplies the credentials; cawir does not enforce provider ToS — it warns where relevant and leaves credential choice to the user.

**Approach.** Hand-rolled HTTP + JSON (`reqwest` + `serde` + `tokio`). No model SDK. The wire protocol is part of what we're learning.

**Multi-provider — grown, not pre-designed.** Rule of Three:
- **v0**: Anthropic-only. No abstraction. Get the agent loop working concretely.
- **v1**: Add OpenAI. *Now* extract a `Provider` trait from the two concrete impls.
- **v2**: Add Ollama. Pressure-test the abstraction.

**Provider and Auth are orthogonal abstractions.** Two separate traits:
- `Provider` — wire format (how to build a request, how to parse a response).
- `AuthMethod` — how credentials attach to an HTTP request (header name/format).

Each provider declares which auth methods it accepts. Matrix:

| Provider | Valid auth methods | Notes |
|---|---|---|
| Anthropic | `ApiKey` | Claude subscription OAuth is **banned** by Anthropic ToS (Feb 20, 2026 update, enforced Apr 4, 2026). cawir deliberately does not implement it. |
| OpenAI | `ApiKey`, `CodexOAuth` | ChatGPT subscription OAuth is **officially supported** by OpenAI for third-party apps. |
| Ollama | `None` | Local, no auth. |

**Credential lookup order** (per provider, when resolving a configured credential):
1. macOS Keychain (crate: `keyring`)
2. Environment variable (`std::env::var`)
3. `.env` file (crate: `dotenvy`)

**Provider selection at startup.** Remember the last-used provider in a config file at the OS-appropriate path (`directories` crate). First launch defaults to the first provider whose credentials are available. Override/change via a `/provider <name>` slash command inside the REPL.

**Streaming.** Not in v0–v2. Later milestone.

**Starting credentials.** User will obtain an Anthropic API key from console.anthropic.com. No OpenAI or Ollama yet — those come with v1 and v2.

### Still to be decided

- _Project layout_: TBD — will follow Cargo defaults until there's a reason not to.
- _Error handling_: TBD — likely start with `Box<dyn Error>`, graduate to `anyhow`/`thiserror` when the tradeoffs are visible.
- _Async runtime_: TBD — `tokio` is the default when async is introduced.

Update this file as real conventions get chosen — don't document guesses.
