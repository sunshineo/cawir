# cawir roadmap

## Philosophy

Build a working coding agent **and** learn Rust — equal goals. Each checkpoint does something cawir couldn't do before, introduces 1–3 new Rust concepts, and can split mid-flight if it's too big.

No speculative abstractions — but the target in [`architecture.md`](architecture.md) tells us where things land when Rule of Three pressure shows up.

## Deliberate simplifications

Some sub-steps use simpler-than-final patterns so the Rust concept of the moment can land cleanly. Each has a planned fix point. Worth knowing up front so the rough edges don't feel like permanent design choices.

- **Fail-fast error handling** (2a–2c). Every fallible call uses `?` and propagates up to `main`. Any failure exits the program. Right for a learning prototype; wrong for a shipped CLI. Per-call graceful recovery lands at **2d (Wire with REPL)** when each user line becomes a Claude call and a single bad response shouldn't end the session. Richer typed error variants land at **2f (Cleanup)** with `thiserror`. Background: `learnings/13-error-handling-fail-fast-vs-graceful.md`.
- **`Box<dyn Error>` as a catch-all** (2a-ii–2e). A pragmatic stepping stone that lets `?` propagate any error type. Replaced by a proper `thiserror` enum at **2f**.
- **`ContentBlock` as a struct, not a tagged enum** (2c–2e). Only handles text blocks; will fail to deserialize tool_use blocks. Grows into a `#[serde(tag = "type")]` enum at **CP3 (Agent loop)** when tool use lands.

## Three phases, nine checkpoints

| Phase | Checkpoints |
|---|---|
| Foundation — not yet an agent | 1. Echo · 2. Chat |
| The agent | 3. Agent loop ⭐ · 4. Modes |
| Craft | 5. Streaming · 6. Multi-model · 7. Hooks · 8. Polyglot · 9. Resume |

---

## 1 — Echo

Minimal REPL with `/exit` and `/help`.

- **1a — First read** *(done 2026-04-23)*. Prompt, read one line, echo, exit. *Rust:* `stdin`, `String`, `Result`, `?`, `print!`+`flush`.
    - *Iterates into:* `print!`/`read_line` is a Surface-layer-only pattern. It swaps out for [Ratatui](https://ratatui.rs) + Crossterm widgets if/when we take the TUI-upgrade seam (post-CP9, speculative). The agent loop underneath is unaffected.
- **1b — Loop forever** *(done 2026-04-23)*. Wrap 1a in `loop`; exit on EOF. *Rust:* `loop`, `break`, EOF detection.
    - *Iterates into:* The `loop { read stdin → dispatch }` shape stays largely unchanged through Chat (CP2) and Agent loop (CP3). At Hooks (CP7) the REPL becomes a `Stream<AgentEvent>` consumer and the loop shifts to event-driven (`while let Some(ev) = stream.next().await`). The outer stdin loop survives above that as a "read user input" pump.
- **1c — Slash commands** *(done 2026-04-23)*. `/exit`, `/help`, else echo. *Rust:* `match` on `&str`, `trim`, `starts_with`.
    - *Iterates into:* The hardcoded `match` extracts into a `Command` trait + `CommandRegistry` around CP6-7, when `/provider <name>` introduces the first slash command with an argument and hook-configured commands add the first dynamic source. Plugin-loaded commands come later (post-CP9 seam). Each current `match` arm maps one-to-one to a future registry entry — the refactor is a lift, not a rewrite.

*Deferred:* `Command` trait + `CommandRegistry` → ~CP6-7 (when `/provider` adds arguments and hook-registered commands add a dynamic source). Plugin-loaded commands → post-CP9. Write the 1c match so each arm is one refactor from a registry lookup.

Components: Surface.

---

## 2 — Chat

Multi-turn conversation with Claude. Biggest Rust jump — seven sub-steps.

- **2a-i — Async entry point** *(done 2026-04-24)*. Add `tokio`; make main `#[tokio::main] async fn`. Behavior unchanged — same prompt, same loop, same exit. *Rust:* adding a crate with feature flags, `#[tokio::main]` proc macro, `async fn` syntax (no `.await` yet).
- **2a-ii — First HTTP call** *(done 2026-04-24)*. Add `reqwest`; fetch a plain-text endpoint (e.g. `api.github.com/zen`) and print it before the REPL loop. Change main's return to `Result<(), Box<dyn Error>>` so `?` can propagate both `io::Error` and `reqwest::Error`. Used `reqwest::Client::builder()` (rather than the simpler `reqwest::get`) so we could set a `User-Agent` header — GitHub's API requires it. *Rust:* the builder pattern, `reqwest::Client`, `.send().await? → .text().await?` chained, `Box<dyn Error>` as a catch-all type-erased error, automatic error conversion via the `From` trait.
    - *Iterates into:* The GitHub Zen demo call gets removed when the first Claude call lands at 2c — replaced, not extended. The `reqwest::Client::builder()` pattern stays (we'll set `Authorization` and other headers the same way for Anthropic). `Box<dyn Error>` persists as a pragmatic stepping stone until 2f, where a `thiserror` enum takes over.
- **2b — Parse JSON** *(done 2026-04-24)*. Fetch `https://api.github.com/repos/rust-lang/rust`, deserialize into a `Repo` struct via `#[derive(Deserialize)]`, print selected fields (name, description, stars, issues, forks). *Rust:* `#[derive(Deserialize)]` with `serde`, `reqwest::Response::json().await?`, `Option<T>` for nullable JSON fields.
    - *Iterates into:* The GitHub repo demo is replaced at 2c when Claude API request/response types take over. The `#[derive(Deserialize)]` pattern persists — reused in every checkpoint from 2c onward, and again at CP5 for stream events (smaller enum variants tagged by a `type` field, one per SSE event, instead of one big response struct).
- **2c — First Claude call** *(done 2026-04-25)*. Hard-coded "hello" POST. cawir reads `ANTHROPIC_API_KEY` from env, builds a `MessageRequest` with the Claude model + a single user `Message`, sends it with `x-api-key` / `anthropic-version` / `content-type` headers, parses the response into `MessageResponse` with `Vec<ContentBlock>`, prints the first block's text. Status check before parsing so 401s give a clear error message instead of a confusing serde failure. *Rust:* `#[derive(Serialize)]`, custom HTTP headers, `std::env::var`, `if let Some(...)`, status-then-body error handling.
    - *Iterates into:* The hard-coded "hello" prompt is replaced by user input at **2d (Wire with REPL)**, where each line becomes a Claude call and `?`-everywhere fail-fast becomes per-call match. Single-shot becomes multi-turn at **2e** with `Vec<Message>` accumulating across turns. The simple `ContentBlock { text: String }` grows into a `#[serde(tag = "type")]` enum at **CP3 (Agent loop)** to also handle tool_use blocks. `Box<dyn Error>` becomes a `thiserror` enum at **2f**.
- **2d — Wire with REPL** *(done 2026-04-27)*. Replaced the hard-coded "hello" with user input from the REPL. Each non-`/command` line goes to Claude as a one-shot prompt; the reply prints; no history yet. Extracted the Claude call into `async fn ask_claude(&Client, &str, &str) -> Result<String, _>`. The call site uses `match` instead of `?` — a network blip or 401 prints to stderr but the REPL keeps running. *Rust:* function extraction with `&` parameters, `async fn` returning `Result`, the fail-fast → graceful transition for runtime errors (setup-time still uses `?`), `eprintln!` for stderr.
    - *Iterates into:* `ask_claude` becomes a method on the `Provider` trait at **CP6 (Multi-model)** once we have a second concrete impl (OpenAI) to extract from. The single-turn no-history behavior becomes multi-turn at **2e** with `Vec<Message>` accumulating across loop iterations. Hardcoded model name and `max_tokens` move to config later, when there is real provider/settings pressure.
- **2e — Multi-turn** *(done 2026-04-27)*. `history: Vec<Message>` accumulates across loop iterations; the full history ships in every API call so Claude has context. User message is pushed before the call, popped if the call fails (Anthropic rejects two consecutive user turns, so we keep history clean on errors). *Rust:* `Vec<T>` mutation patterns (`push`, `pop`), `Vec::new()` vs `vec![]`, slice parameters (`&[Message]` instead of `&Vec<Message>`), `Clone` derive, `.to_vec()` to clone a slice into an owned `Vec`.
    - *Iterates into:* The history is in-memory only; persists across `/exit` + restart at **CP9 (Resume)** by serializing `Vec<Message>` to disk. `Message`'s `content: String` gets enriched at **CP3 (Agent loop)** — once tool_use blocks land, content can't flatten to a string anymore. The unbounded growth of `history` will eventually need compaction (currently a "Beyond CP9" speculative seam in the architecture).
- **2f — Cleanup** *(done 2026-04-28)*. Split the one-file prototype into `main.rs` + `lib.rs` + `session.rs` + `error.rs`; added `thiserror`; replaced `Box<dyn Error>` with a typed app error enum and project `Result<T>` alias. Added `AGENTS.md -> CLAUDE.md` so Codex reads the same project guidance. *Rust:* binary vs library crates, `mod`, `pub`, `pub use`, type aliases, `#[derive(thiserror::Error)]`, `#[error(...)]`, `#[from]`, `std::convert::From`, how `?` converts errors, the Rust prelude (`Debug`, `From`).
    - *Iterates into:* `Message` now lives in `session.rs` as pure conversation data, ready to grow toward CP9 persistence. `ask_claude` remains a concrete Anthropic function; it becomes a provider method only after a second provider creates Rule-of-Three pressure at **CP6 (Multi-model)**. The error enum can now gain variants as CP3 introduces file IO, shell execution, tool dispatch, and permission failures.

Components: Surface, Core engine (minimal), External (hard-coded Anthropic call, no `Provider` trait yet).

---

## 3 — Agent loop ⭐

The soul of cawir. Three tools (`read_file`, `write_file`, `shell`) dispatched in a `match`; loop handles the call-execute-feedback cycle until the model stops calling tools.

*Rust:* the agent-loop protocol itself, `serde_json::Value`, `std::fs`, `std::process::Command`, `match`-based dispatch, hardcoded inline approval for mutating tools.

*Deferred:*
- `Tool` trait + `ToolRegistry` → ~CP6-7 (Rule of Three with concrete pressure)
- `PermissionMode` enum + `/mode` → CP4
- Plan mode + `ExitPlanMode` → CP4
- Multi-source tool registration (plugins/MCP) → post-CP9

Write dispatch so each arm is already the future trait method's signature.

Done when: "read Cargo.toml and tell me the deps" works end-to-end; multi-step tasks with reads + approved writes complete.

Components: Core engine, Surface.

---

## 4 — Modes

`PermissionMode` enum + `/mode <name>`. Plan mode: Claude researches read-only, calls `ExitPlanMode`, REPL prompts for approval.

*Rust:* `enum` as state machine, exhaustive `match`, event-emitting tools (not-mutating).

Done when: `/mode plan` restricts writes; `exit_plan_mode` approval flow works; `/mode default` / `/mode bypass` work.

Components: Policy.

---

## 5 — Streaming

Claude's output appears token-by-token via SSE.

*Rust:* `futures::Stream`, `StreamExt`, SSE parsing, partial-JSON handling.

Done when: responses stream visibly; tool calls still work mid-stream.

Components: External, Core engine.

---

## 6 — Multi-model

Add OpenAI. Extract `Provider` trait from two concrete impls. Natural moment to also extract `Tool` trait from CP3's three functions.

*Rust:* **extracting a trait from two impls**, static vs dynamic dispatch, default trait methods.

Done when: `/provider openai` and `/provider anthropic` both work.

Components: External.

---

## 7 — Hooks

Event bus. Agent loop emits `AgentEvent`s; `HookRegistry` dispatches to handlers loaded from `settings.json`. Demo: `cargo fmt` after `write_file` on `.rs`.

*Rust:* `Arc<Mutex<T>>` or `tokio::sync::RwLock`, channels, async dispatch, settings.json merge (user → project → local).

Done when: configured hook fires; `PreToolUse::Deny` blocks a tool.

Components: Core engine, Capabilities, External (SettingsResolver arrives).

---

## 8 — Polyglot

Add Ollama. `AuthMethod` trait orthogonal to `Provider`. Credential chain: Keychain → env → `.env`.

*Rust:* second orthogonal trait composing with Provider, `keyring`, `dotenvy`.

Done when: Ollama works with no credentials; OpenAI works from env or Keychain; mid-session provider switching works.

Components: External.

---

## 9 — Resume

`cawir --resume <id>` and `cawir --continue`.

*Rust:* serde to/from disk, `clap`, `directories` crate, filesystem basics.

Done when: a conversation survives `/exit` + restart.

Components: Core engine, Surface.

---

## Rust concepts by checkpoint

| # | Checkpoint | New Rust concepts |
|---|---|---|
| 1 | Echo | `String`/`&str`, `stdin`, `Result`, `?`, `loop`, `match` |
| 2 | Chat | crates, `async`/`await`, serde derive, `Vec`, structs/enums, modules, `thiserror`, env vars |
| 3 | Agent loop | agent-loop protocol, `serde_json::Value`, `std::fs`, `Command`, match dispatch |
| 4 | Modes | `enum` state machine, exhaustive match, special tools |
| 5 | Streaming | `futures::Stream`, SSE parsing, partial JSON |
| 6 | Multi-model | **extracting a trait**, static vs dynamic dispatch |
| 7 | Hooks | `Arc`/`Mutex`, channels, async dispatch, JSON merge |
| 8 | Polyglot | orthogonal trait, `keyring`, `dotenvy` |
| 9 | Resume | serde disk I/O, `clap`, `directories` |

## Beyond Checkpoint 9 (speculative)

MCP tools · plugin loading · subagents · auto-mode classifier · context compaction · memory extraction · TUI upgrade ([Ratatui](https://ratatui.rs) + Crossterm) · remote transport.

Each has a seam in [`architecture.md`](architecture.md). The TUI upgrade specifically is a Surface-layer swap: Ratatui consumer replaces the `println!`-based REPL, both consuming the same `Stream<AgentEvent>`. Agent loop and everything below stay identical.
