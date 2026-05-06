# cawir — Coding Agent Written in Rust

## What this project is

A minimal coding agent, written in Rust, built incrementally as a learning vehicle. Two goals held equally:

1. **Learn Rust** — the author is new to Rust. Every step should teach a Rust concept, not just produce working code.
2. **Learn how coding agents work** — by building one from scratch (model loop, tool use, permissions, context), not just using one.

The target is *not* to compete with Claude Code or Cursor. The target is a small, readable agent the author fully understands.

## Project docs

- [`docs/status.md`](docs/status.md) tracks the current implementation state and what is next.
- [`docs/roadmap.md`](docs/roadmap.md) defines the checkpoint sequence and scope.
- [`docs/architecture.md`](docs/architecture.md) describes the target component design and extension seams.
- [`docs/testing.md`](docs/testing.md) lists offline and live provider test commands.
- `learnings-rust/*.md` stores durable Rust notes organized by topic.
- `learnings-agent/*.md` stores durable coding-agent design notes organized by topic.

## How to collaborate on this project

### Teach as you go

The author does not know Rust. When you write or review code here:

- **Explain new Rust concepts inline** the first time they appear — ownership, borrowing, lifetimes (`'a`), traits, `impl`, `Result<T, E>`, `Option<T>`, `?`, `async`/`await`, `match`, pattern binding, modules, `Cargo.toml`, crates, `derive` macros, error types, `Box`/`Arc`/`Rc`, `&str` vs `String`.
- **Contrast with other languages** briefly when it helps. The author has general programming experience; mapping Rust concepts to their analogues elsewhere is useful.
- **Prefer the idiomatic Rust way** and say *why* it's idiomatic — don't just translate from another language.
- **One idea per step.** If a change introduces a new language feature *and* a new architectural concept, split them.

### Grow the codebase organically

Do not implement the architecture all at once. The [target architecture](docs/architecture.md) is known — five functional groups, typed event bus, serializable sessions, provider/auth split — but each checkpoint ships with only the minimum that works. The first version is probably ~100 lines with no trait abstraction yet. Each subsequent checkpoint extracts one more component from the target, informed by concrete code, not by planning ahead.

The discipline: **no speculative abstractions, but the target shape is known.** That's the difference between "grown organically" and "improvised." When we defer a `Provider` trait while there is still only one provider, we still know where it will live once a second concrete implementation creates real extraction pressure — we're not guessing.

### Code style

- No unused features, no speculative abstractions. Three similar lines beats a premature trait.
- Comments only when the *why* is non-obvious. Rust's type system already documents most *what*s.
- When introducing a new crate, explain what it does and why this crate vs alternatives (briefly).

### Implementation workflow — write, review, learn, commit

For each checkpoint (or sub-step) from [`docs/roadmap.md`](docs/roadmap.md):

1. **Write the code.** Apply the edits; do NOT commit yet. The user reads the diff in VS Code.
2. **Discuss.** The user asks about anything unclear — syntax, idioms, why-this-not-that. This is where Rust concepts get taught in context.
   After writing each checkpoint or sub-step, proactively propose a short list of concrete discussion prompts tied to the diff. Prefer 2-5 prompts that point at the new Rust concepts, ownership or borrowing choices, data-shape decisions, and why this step stayed concrete instead of abstract.
3. **Save learnings.** Significant Rust discussion points get distilled into `learnings-rust/*.md`; significant coding-agent design points get distilled into `learnings-agent/*.md`. Organize by topic, not chronology.
4. **Commit.** Once the user is satisfied and learnings are saved, commit with a message that references the checkpoint (e.g. `feat: 1a — first read`).

Every implementation step is a teaching opportunity. The trails of `learnings-rust/` and `learnings-agent/` files are the durable records of what's been learned.

## Conventions

### Toolchain (decided 2026-04-23)

- **Installer:** `rustup` (official), installed via `curl https://sh.rustup.rs | sh`. Not Homebrew.
- **Channel:** `stable`. No `rust-toolchain.toml` pin yet — add one only when we hit a reason.
- **Profile:** `default` — ships `rustc`, `cargo`, `rust-std`, `rust-docs`, `rustfmt`, `clippy`. Use all four tools from day one.
- **PATH:** `~/.cargo/bin` added via `~/.zshenv` (not `~/.zshrc`) so non-interactive shells/cron/launchd/subprocesses also see cargo.
- **Linter policy:** run `cargo clippy` regularly while learning — its suggestions are one of the best free Rust tutors. `cargo fmt` before committing.

### Testing

Always run the offline checks before finishing code changes:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Live provider smoke tests are ignored by default because they use real credentials, network access, and provider quota. When changing provider wire format, auth resolution, credential refresh, or request/response parsing, run the relevant matrix command:

```sh
PROVIDER=anthropic AUTH_OPTION=api-key cargo test live_smoke -- --ignored --nocapture
PROVIDER=openai AUTH_OPTION=api-key cargo test live_smoke -- --ignored --nocapture
PROVIDER=openai AUTH_OPTION=codex-oauth cargo test live_smoke -- --ignored --nocapture
```

The live test uses the same credential lookup order as the REPL: `credentials.json` → environment → `.env`.

### Editor / IDE (decided 2026-04-23)

- **Editor:** VS Code.
- **LSP:** `rust-analyzer` VS Code extension (publisher: *The Rust Programming Language*, id `rust-lang.rust-analyzer`). Extension auto-downloads the rust-analyzer binary on first `.rs` file open. Do **not** install the deprecated `rust-lang.rust` (RLS-based) extension.
- **Debugger:** `CodeLLDB` extension (id `vadimcn.vscode-lldb`). Wraps LLDB via DAP. Understands Rust pretty-printers (`Vec`, `Option`, `Result`).
- **Editor config in repo:** not committed. Solo learning project — keep the repo clean. Revisit if collaboration starts.

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
1. OS-appropriate `credentials.json` config file (`directories` crate; `0600` on Unix)
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

Update this file when collaboration rules or durable project conventions change. Put day-to-day progress in `docs/status.md`, not here.
