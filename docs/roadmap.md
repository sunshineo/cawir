# cawir roadmap

## Philosophy

Build a working coding agent **and** learn Rust — equal goals. Each checkpoint does something cawir couldn't do before, introduces 1–3 new Rust concepts, and can split mid-flight if it's too big.

No speculative abstractions — but the target in [`architecture.md`](architecture.md) tells us where things land when Rule of Three pressure shows up.

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
- **2a-ii — First HTTP call.** Add `reqwest`; fetch a plain-text endpoint (e.g. `api.github.com/zen`) and print it before the REPL loop. Change main's return to `Result<(), Box<dyn Error>>` so `?` can propagate both `io::Error` and `reqwest::Error`. *Rust:* `reqwest::get`, chained `.await?`, `Box<dyn Error>` as a catch-all type-erased error, automatic error conversion via the `From` trait.
- **2b — Parse JSON.** Fetch a JSON endpoint, deserialize into a struct. *Rust:* `#[derive(Deserialize)]`, `.json::<T>().await?`.
- **2c — First Claude call.** Hard-coded "hello" POST. *Rust:* `#[derive(Serialize)]`, custom headers, `std::env::var`.
- **2d — Wire with REPL.** User input → Claude → print. Single-turn. *Rust:* combining sync loop with async.
- **2e — Multi-turn.** `Vec<Message>` across turns. *Rust:* `Vec`, struct/enum variant data.
- **2f — Cleanup.** `main.rs` + `lib.rs` + `session.rs` + `error.rs`; `thiserror`. *Rust:* `mod`, `pub`, `use`, `#[derive(thiserror::Error)]`, `#[from]`.

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
