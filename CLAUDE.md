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

Do not implement the architecture all at once. The [target architecture](docs/architecture.md) is known — 10 layers, typed event bus, serializable sessions, provider/auth split — but each version ships with only the minimum that works. v0.1 is probably ~100 lines with no trait abstraction yet. Each subsequent version extracts one more layer from the target, informed by concrete code, not by planning ahead.

The discipline: **no speculative abstractions, but the target shape is known.** That's the difference between "grown organically" and "improvised." When we defer a `Provider` trait at v0.1, we know where it will live when we extract it from two concrete impls at v0.6 — we're not guessing.

### Code style

- No unused features, no speculative abstractions. Three similar lines beats a premature trait.
- Comments only when the *why* is non-obvious. Rust's type system already documents most *what*s.
- When introducing a new crate, explain what it does and why this crate vs alternatives (briefly).

### Implementation workflow — write, review, learn, commit

For each checkpoint (or sub-step) from [`docs/roadmap.md`](docs/roadmap.md):

1. **Write the code.** Apply the edits; do NOT commit yet. The user reads the diff in VS Code.
2. **Discuss.** The user asks about anything unclear — syntax, idioms, why-this-not-that. This is where Rust concepts get taught in context.
3. **Save learnings.** Significant discussion points get distilled into a `learnings/*.md` file, organized by topic (not by chronology).
4. **Commit.** Once the user is satisfied and learnings are saved, commit with a message that references the checkpoint (e.g. `feat: 1a — first read`).

Every implementation step is a teaching opportunity. The trail of `learnings/` files is the durable record of what's been learned.

## Current state (as of 2026-04-23)

- Toolchain, editor, and provider strategy decided and in place (see Conventions below).
- `cargo init --bin` scaffold committed; `cargo run` produces "Hello, world!".
- [Target architecture](docs/architecture.md) committed — 5-component-group design.
- [Roadmap](docs/roadmap.md) committed — 9 named checkpoints across three phases (Foundation → The agent → Craft).
- **Checkpoint 1 (Echo) done** (2026-04-23): 1a, 1b, 1c. REPL loops, handles `/exit`, `/help`, rejects unknown `/commands`, echoes everything else.
- **2a-i — Async entry point** done (2026-04-24). First crate added (`tokio`); main is now `#[tokio::main] async fn`.
- **2a-ii — First HTTP call** done (2026-04-24). Second crate added (`reqwest`). First `.await` on a network call. Main return type is now `Result<(), Box<dyn Error>>`.
- **2b — Parse JSON** done (2026-04-24). Third crate added (`serde`). cawir fetches `api.github.com/repos/rust-lang/rust`, deserializes into a `Repo` struct via `#[derive(Deserialize)]`, prints name / description / stars / issues / forks.
- **2c — First Claude call** done (2026-04-25). Hard-coded "hello" POST to Anthropic; reads `ANTHROPIC_API_KEY` from env.
- **2d — Wire with REPL** done (2026-04-27). REPL now sends each non-`/command` line to Claude (single-turn, no history). Claude call extracted into `ask_claude`; per-call match means a single bad response doesn't end the session.
- Ten learnings notes so far: `05-ownership-and-borrowing`, `06-traits-and-scope`, `07-result-question-mark-unit`, `08-match-patterns-and-bindings`, `09-async-tokio-and-runtime`, `10-rust-async-vs-other-languages`, `11-derive-macros`, `12-serde-deserialization`, `13-error-handling-fail-fast-vs-graceful`, `14-function-parameters-and-ownership`.
- Next: **2e — Multi-turn** (maintain `Vec<Message>` across loop iterations so Claude remembers earlier turns).

## Architecture

cawir is a **component graph** centered on the agent loop, organized into five functional groups: **Surface** (REPL + slash commands), **Core engine** (agent loop + session + prompt + event dispatch), **Capabilities** (tool registry, hook registry), **Policy** (permission modes + validators), and **External** (provider, auth, credential chain, settings resolver). It is not a layered stack — the agent loop at the center fans out to multiple components; the REPL consumes agent events alongside parsing slash commands locally.

Properties locked in from v0.1:

- **Async + streaming loop.** `agent::run` returns a `Stream<AgentEvent>`; the REPL consumes it.
- **Typed lifecycle events.** `AgentEvent` enum (`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, …) dispatched synchronously to hooks (who can modify/deny) and asynchronously into the REPL stream (observation only).
- **Self-describing tools.** Each tool is a `Tool` trait impl declaring name, JSON schema, description, and execution logic.
- **Permission = mode + per-tool validator + PreToolUse hook.** Modes: `Default`, `Plan`, `AcceptEdits`, `Bypass`.
- **Provider ⊥ Auth.** Separate traits. Each provider declares which auth methods it accepts.
- **Session is pure data.** `Session` derives `Serialize`/`Deserialize` from v0.1; `/resume` later becomes an implementation, not a schema migration.
- **One `SettingsResolver`.** User → project → local merge, used everywhere config is read.

Full detail — the component flow diagram, each group's types/traits, extension seams, target module layout, and the 10 commit-level decisions — lives in [`docs/architecture.md`](docs/architecture.md).

Design influences: we studied Claude Code's community deep-dive docs and source to borrow its component decomposition, explicitly skipping complexity that's about Anthropic's production scale (Bash AST parsing, LLM permission classifiers, multi-layer compaction) rather than about what a coding agent fundamentally is.

## Roadmap

[`docs/roadmap.md`](docs/roadmap.md) sequences the work into nine named checkpoints across three phases:

1. **Echo** · 2. **Chat** — *Foundation: not yet an agent*
3. **Agent loop** ⭐ · 4. **Modes** — *The agent: the heart of the project*
5. **Streaming** · 6. **Multi-model** · 7. **Hooks** · 8. **Polyglot** · 9. **Resume** — *Craft: making it good*

Each checkpoint pairs a user-visible capability with 1–3 new Rust concepts. Checkpoint 3 ("Agent loop") is the moment cawir becomes a real coding agent — read + write + shell tools in the dispatch/execute/feedback cycle that defines every coding-agent product.

The roadmap is a living document — checkpoints will split, merge, and reorder as we actually implement.

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

### Other conventions (decided in architecture doc)

- **Project layout:** per `docs/architecture.md` target module layout. Early versions start with a subset.
- **Error handling:** `thiserror` enum from v0.1. Not `anyhow`, not `Box<dyn Error>`.
- **Async runtime:** `tokio`.
- **Serde discipline:** every type in `Session` derives `Serialize`/`Deserialize` from v0.1 (even before `/resume` ships).

Update this file as new conventions emerge — don't document guesses.
