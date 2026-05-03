# cawir roadmap

## Philosophy

Build a working coding agent **and** learn Rust — equal goals. Each checkpoint does something cawir couldn't do before, introduces 1–3 new Rust concepts, and can split mid-flight if it's too big.

No speculative abstractions — but the target in [`architecture.md`](architecture.md) tells us where things land when Rule of Three pressure shows up.

Current completion state lives in [`status.md`](status.md). This document defines checkpoint intent and sequence.

## Deliberate simplifications

Some sub-steps use simpler-than-final patterns so the Rust concept of the moment can land cleanly. Each has a planned fix point. Worth knowing up front so the rough edges don't feel like permanent design choices.

- **Fail-fast error handling** (2a–2c). Every fallible call uses `?` and propagates up to `main`. Any failure exits the program. Right for a learning prototype; wrong for a shipped CLI. Per-call graceful recovery lands at **2d (Wire with REPL)** when each user line becomes a Claude call and a single bad response shouldn't end the session. Richer typed error variants land at **2f (Cleanup)** with `thiserror`. Background: `learnings-rust/13-error-handling-fail-fast-vs-graceful.md`.
- **`Box<dyn Error>` as a catch-all** (2a-ii–2e). A pragmatic stepping stone that lets `?` propagate any error type. Replaced by a proper `thiserror` enum at **2f**.
- **`ContentBlock` as a struct, not a tagged enum** (2c–2e). Only handles text blocks; will fail to deserialize tool_use blocks. Grows into a `#[serde(tag = "type")]` enum at **3b (Parse tool-use responses)**.
- **Tool input schemas as raw `serde_json::Value`** (starting at 3a). First tool schemas are written as JSON literals with `json!` because that mirrors Anthropic's docs and keeps CP3 focused on the agent loop, not on designing a Rust model of JSON Schema. Flexible, but light on compile-time checking. If several tools make schema repetition or schema typos a real problem, extract small typed schema helpers from that concrete pressure rather than inventing a mini schema type system upfront.

## Three phases, nine checkpoints

| Phase | Checkpoints |
|---|---|
| Foundation — not yet an agent | 1. Echo · 2. Chat |
| The agent | 3. Agent loop ⭐ · 3.5. Refactor shape · 4. Modes |
| Craft | 5. Streaming · 6. Multi-model · 7. Hooks · 8. Polyglot · 9. Resume |

---

## 1 — Echo

Minimal REPL with `/exit` and `/help`.

- **1a — First read**. Prompt, read one line, echo, exit. *Rust:* `stdin`, `String`, `Result`, `?`, `print!`+`flush`.
    - *Iterates into:* `print!`/`read_line` is a Surface-layer-only pattern. It swaps out for [Ratatui](https://ratatui.rs) + Crossterm widgets if/when we take the TUI-upgrade seam (post-CP9, speculative). The agent loop underneath is unaffected.
- **1b — Loop forever**. Wrap 1a in `loop`; exit on EOF. *Rust:* `loop`, `break`, EOF detection.
    - *Iterates into:* The `loop { read stdin → dispatch }` shape stays largely unchanged through Chat (CP2) and Agent loop (CP3). At Hooks (CP7) the REPL becomes a `Stream<AgentEvent>` consumer and the loop shifts to event-driven (`while let Some(ev) = stream.next().await`). The outer stdin loop survives above that as a "read user input" pump.
- **1c — Slash commands**. `/exit`, `/help`, else echo. *Rust:* `match` on `&str`, `trim`, `starts_with`.
    - *Iterates into:* The hardcoded `match` extracts into a `Command` trait + `CommandRegistry` around CP6-7, when `/provider <name>` introduces the first slash command with an argument and hook-configured commands add the first dynamic source. Plugin-loaded commands come later (post-CP9 seam). Each current `match` arm maps one-to-one to a future registry entry — the refactor is a lift, not a rewrite.

*Deferred:* `Command` trait + `CommandRegistry` → ~CP6-7 (when `/provider` adds arguments and hook-registered commands add a dynamic source). Plugin-loaded commands → post-CP9. Write the 1c match so each arm is one refactor from a registry lookup.

Components: Surface.

---

## 2 — Chat

Multi-turn conversation with Claude. Biggest Rust jump — seven sub-steps.

- **2a-i — Async entry point**. Add `tokio`; make main `#[tokio::main] async fn`. Behavior unchanged — same prompt, same loop, same exit. *Rust:* adding a crate with feature flags, `#[tokio::main]` proc macro, `async fn` syntax (no `.await` yet).
- **2a-ii — First HTTP call**. Add `reqwest`; fetch a plain-text endpoint (e.g. `api.github.com/zen`) and print it before the REPL loop. Change main's return to `Result<(), Box<dyn Error>>` so `?` can propagate both `io::Error` and `reqwest::Error`. Used `reqwest::Client::builder()` (rather than the simpler `reqwest::get`) so we could set a `User-Agent` header — GitHub's API requires it. *Rust:* the builder pattern, `reqwest::Client`, `.send().await? → .text().await?` chained, `Box<dyn Error>` as a catch-all type-erased error, automatic error conversion via the `From` trait.
    - *Iterates into:* The GitHub Zen demo call gets removed when the first Claude call lands at 2c — replaced, not extended. The `reqwest::Client::builder()` pattern stays (we'll set `Authorization` and other headers the same way for Anthropic). `Box<dyn Error>` persists as a pragmatic stepping stone until 2f, where a `thiserror` enum takes over.
- **2b — Parse JSON**. Fetch `https://api.github.com/repos/rust-lang/rust`, deserialize into a `Repo` struct via `#[derive(Deserialize)]`, print selected fields (name, description, stars, issues, forks). *Rust:* `#[derive(Deserialize)]` with `serde`, `reqwest::Response::json().await?`, `Option<T>` for nullable JSON fields.
    - *Iterates into:* The GitHub repo demo is replaced at 2c when Claude API request/response types take over. The `#[derive(Deserialize)]` pattern persists — reused in every checkpoint from 2c onward, and again at CP5 for stream events (smaller enum variants tagged by a `type` field, one per SSE event, instead of one big response struct).
- **2c — First Claude call**. Hard-coded "hello" POST. cawir reads `ANTHROPIC_API_KEY` from env, builds a `MessageRequest` with the Claude model + a single user `Message`, sends it with `x-api-key` / `anthropic-version` / `content-type` headers, parses the response into `MessageResponse` with `Vec<ContentBlock>`, prints the first block's text. Status check before parsing so 401s give a clear error message instead of a confusing serde failure. *Rust:* `#[derive(Serialize)]`, custom HTTP headers, `std::env::var`, `if let Some(...)`, status-then-body error handling.
    - *Iterates into:* The hard-coded "hello" prompt is replaced by user input at **2d (Wire with REPL)**, where each line becomes a Claude call and `?`-everywhere fail-fast becomes per-call match. Single-shot becomes multi-turn at **2e** with `Vec<Message>` accumulating across turns. The request grows a first concrete tool at **3a (First tool advertised)**, and the simple `ContentBlock { text: String }` grows into a `#[serde(tag = "type")]` enum at **3b (Parse tool-use responses)**. `Box<dyn Error>` becomes a `thiserror` enum at **2f**.
- **2d — Wire with REPL**. Replace the hard-coded "hello" with user input from the REPL. Each non-`/command` line goes to Claude as a one-shot prompt; the reply prints; no history yet. Extract the Claude call into `async fn ask_claude(&Client, &str, &str) -> Result<String, _>`. The call site uses `match` instead of `?` — a network blip or 401 prints to stderr but the REPL keeps running. *Rust:* function extraction with `&` parameters, `async fn` returning `Result`, the fail-fast → graceful transition for runtime errors (setup-time still uses `?`), `eprintln!` for stderr.
    - *Iterates into:* `ask_claude` becomes a method on the `Provider` trait at **CP6 (Multi-model)** once we have a second concrete impl (OpenAI) to extract from. The single-turn no-history behavior becomes multi-turn at **2e** with `Vec<Message>` accumulating across loop iterations. Hardcoded model name and `max_tokens` move to config later, when there is real provider/settings pressure.
- **2e — Multi-turn**. `history: Vec<Message>` accumulates across loop iterations; the full history ships in every API call so Claude has context. Push the user message before the call, and pop it if the call fails (Anthropic rejects two consecutive user turns, so history must stay clean on errors). *Rust:* `Vec<T>` mutation patterns (`push`, `pop`), `Vec::new()` vs `vec![]`, slice parameters (`&[Message]` instead of `&Vec<Message>`), `Clone` derive, `.to_vec()` to clone a slice into an owned `Vec`.
    - *Iterates into:* The history is in-memory only; persists across `/exit` + restart at **CP9 (Resume)** by serializing `Vec<Message>` to disk. `Message`'s `content: String` gets enriched at **3e (Send one tool result back)** — once assistant tool-use blocks and tool results land, content can't flatten to a string anymore. The unbounded growth of `history` will eventually need compaction (currently a "Beyond CP9" speculative seam in the architecture).
- **2f — Cleanup**. Split the one-file prototype into `main.rs` + `lib.rs` + `session.rs` + `error.rs`; add `thiserror`; replace `Box<dyn Error>` with a typed app error enum and project `Result<T>` alias. Add `AGENTS.md -> CLAUDE.md` so Codex reads the same project guidance. *Rust:* binary vs library crates, `mod`, `pub`, `pub use`, type aliases, `#[derive(thiserror::Error)]`, `#[error(...)]`, `#[from]`, `std::convert::From`, how `?` converts errors, the Rust prelude (`Debug`, `From`).
    - *Iterates into:* `Message` now lives in `session.rs` as pure conversation data, ready to grow toward CP9 persistence. `ask_claude` remains a concrete Anthropic function; it becomes a provider method only after a second provider creates Rule-of-Three pressure at **CP6 (Multi-model)**. The error enum can now gain variants as CP3 introduces file IO, shell execution, tool dispatch, and permission failures.

Components: Surface, Core engine (minimal), External (hard-coded Anthropic call, no `Provider` trait yet).

---

## 3 — Agent loop ⭐

The soul of cawir. cawir stops being "chat with Claude" and becomes a real coding agent: first by advertising one concrete tool, then by handling `tool_use`, then by executing approved local actions, and only then by looping until Claude stops.

- **3a — First tool advertised**. Add a single concrete tool, `read_file`, to the Anthropic request so Claude can reach for a real tool instead of only answering in text. This step is allowed to expose the next failure mode: if Claude emits `tool_use`, the old parser may break, and **3b** fixes that. *Rust:* extending request structs, `serde_json::Value`, the `json!` macro, hand-built JSON schema data.
    - *Iterates into:* The first tool stays concrete and inline. Its schema is allowed to be raw `serde_json::Value` at first because that mirrors the provider docs directly. If several tools make that too repetitive or too typo-prone, extract small typed schema helpers from the concrete repetition. The tool definition only extracts into a `Tool` trait + registry at **CP6-7**, once there is real pressure from multiple concrete tools and providers.
- **3b — Parse tool-use responses**. Replace the text-only `ContentBlock` parsing with a tagged enum that handles both `text` and `tool_use`. cawir can now receive a tool request without deserialization failure. *Rust:* `#[serde(tag = "type")]`, data-carrying enums, `match` on enum variants.
    - *Iterates into:* The same tagged-enum deserialization pattern comes back at **CP5 (Streaming)**, where SSE events also arrive as smaller typed variants keyed by a `type` field.
- **3c — Execute one read-only tool call**. Match on a parsed `read_file` tool call, extract its `path`, run the local file read, and surface the raw result. *Rust:* `serde_json::Value`, simple input extraction, `std::fs::read_to_string`, `match` dispatch.
    - *Iterates into:* The plain `match` dispatcher is deliberate. Each arm should already look like the future trait method signature, but no `Tool` trait or registry is extracted yet.
- **3d — Second read-only tool: list_files**. Add `list_files` so Claude can inspect repository or folder structure before choosing files to read. Match on a parsed `list_files` tool call, extract its `path`, run a directory listing, and surface the raw result. *Rust:* `std::fs::read_dir`, `DirEntry`, collecting and formatting owned `String` output.
    - *Iterates into:* This stays concrete and inline beside `read_file`. Together they make the read-only inspection path more useful before any approval system exists, while still keeping dispatch as a plain `match`.
- **3e — Send one tool result back**. Enrich session or message data enough to store assistant tool-use content and a `tool_result`, then send the `read_file` or `list_files` result back and print Claude's follow-up answer. *Rust:* richer serde enums or structs for conversation state, owned `Vec` data, cloning and ownership across request assembly.
    - *Iterates into:* This is the moment `Message` stops flattening to plain text. The same "session is pure serde data" discipline later pays off at **CP9 (Resume)**.
- **3f — Repeat until Claude stops**. Turn the one-shot call-execute-feedback flow into the real read-only agent loop: keep executing `read_file` and `list_files` calls until Claude returns a normal stop. *Rust:* loop control, stop conditions, repeated request or response cycles.
    - *Iterates into:* The control-flow shape here is the foundation for later streaming and event emission. CP5 and CP7 change how the loop surfaces work, not the fact that the loop owns orchestration.
- **3g — Tool failures become tool results**. Missing files and invalid tool inputs are returned to Claude as tool errors instead of crashing or corrupting session history. *Rust:* error-to-data conversion, keeping state consistent after partial failure.
    - *Iterates into:* CP4 adds policy errors with structure; CP7 adds hook-driven denial. CP3 only needs the simpler rule that a tool failure should stay inside the conversation, not tear down the session.
- **3h — First mutating tool: write_file with inline approval**. Add `write_file`, but gate it with a direct REPL approval prompt before execution. Denials and write failures flow back through the tool-result path from 3g. *Rust:* `std::fs::write`, inline approval flow, distinguishing denial from execution failure.
    - *Iterates into:* The approval check is intentionally hardcoded here so mutating tools can land safely before the real policy system. `PermissionMode` still starts at **CP4 (Modes)**.
- **3i — Second mutating tool: shell with inline approval**. Add `shell`, also behind direct approval, and return stdout, stderr, denial, or execution failure into the tool-result path. *Rust:* `std::process::Command`, exit-status handling, stdout or stderr capture.
    - *Iterates into:* Shell approval remains a local REPL concern in CP3. Finer-grained policy and hook-based vetoes still wait for **CP4** and **CP7**.

*Deferred:*
- `Tool` trait + `ToolRegistry` → ~CP6-7 (Rule of Three with concrete pressure)
- `PermissionMode` enum + `/mode` → CP4
- Plan mode + `ExitPlanMode` → CP4
- Multi-source tool registration (plugins/MCP) → post-CP9
- Tool-output budgeting → later CP4 cleanup. Add file-size caps or ranged reads so one `read_file` cannot inject a huge file into history by default. Add stdout/stderr byte or line caps for `shell`, with clear truncation markers; Checkpoint 3i currently decodes process output into one lossy UTF-8 string for simplicity.
- Rate-limit recovery → later External cleanup. Parse 429 `retry-after` / rate-limit headers and decide whether to wait, retry, or return a clearer recoverable error.
- Cache observability → later External cleanup. Parse Anthropic `usage` fields such as `input_tokens`, `cache_creation_input_tokens`, and `cache_read_input_tokens` so cache behavior is visible while learning.

Write dispatch so each arm is already the future trait method's signature.

Done when: "read Cargo.toml and tell me the deps" works end-to-end; approved writes and shell commands work; one user prompt can trigger multiple tool calls; and tool failures stay inside the conversation instead of killing the session.

Components: Core engine, Surface.

---

## 3.5 — Refactor shape

Checkpoint 3 made cawir a real agent, but `lib.rs` now owns too many responsibilities: Anthropic HTTP, tool definitions, tool execution, approval prompts, the agent loop, slash commands, and most tests. Before adding permission modes, split the code by responsibility while preserving behavior.

This is an organizational checkpoint, not an abstraction checkpoint. The goal is clearer modules, not a provider trait, tool trait, command registry, plugin system, or config system.

- **3.5a — Move Anthropic API code to `anthropic.rs`**. Move request/response structs and the concrete `ask_claude` call out of `lib.rs`. Keep the implementation Anthropic-specific. *Rust:* `pub(crate)` visibility, module imports, separating wire protocol code from orchestration.
    - *Iterates into:* A future `Provider` trait still waits until CP6 when OpenAI adds real extraction pressure. This step only gives the concrete Anthropic code a home.
- **3.5b — Move tools to `tools.rs`**. Move tool schemas, tool dispatch, tool execution, approval prompts, and tool tests out of `lib.rs`. Add a small concrete `definitions() -> Vec<ToolDefinition>` and `execute_tool_uses(...)` surface, but keep dispatch as a plain `match`. *Rust:* private helpers inside a module, public module functions, tests living beside the code they exercise.
    - *Iterates into:* A `Tool` trait + registry still waits until CP6-7. This step makes the current match registry-shaped without introducing dynamic dispatch. `execute_tool_uses(...)` is allowed to be a slightly orchestration-heavy boundary for now because it keeps the 3.5b call site simple. When the agent loop and event handling grow, expect it to split: `tools.rs` keeps the low-level "execute one named tool with JSON input" responsibility, while `agent.rs` owns turning model `tool_use` blocks into events, tool results, and session updates.
- **3.5c — Move agent loop to `agent.rs`**. Move `run_agent_turn`, `MAX_TOOL_ROUNDS`, and loop orchestration out of `lib.rs`. The agent loop calls the concrete Anthropic module and the concrete tools module. *Rust:* borrowing `&mut Vec<Message>` through orchestration, module boundaries around control flow.
    - *Iterates into:* CP5 streaming and CP7 hooks will change how the loop emits progress, but this step preserves the current request-tool-result loop.
- **3.5d — Move REPL and slash commands to `repl.rs`**. Move `run`, `print_help`, startup setup, and the slash-command `match` out of `lib.rs`. Leave `/exit` and `/help` concrete. *Rust:* library exports, binary crate calling `cawir::run`, keeping surface code separate from engine code.
    - *Iterates into:* `/mode` in CP4 will extend the concrete slash-command match before any command registry exists. Later Surface implementations can sit beside or replace `repl.rs`: a richer TUI, a one-shot `exec` command, stdio protocol mode, or a WebSocket server.

Expected shape after 3.5:

```text
src/
  main.rs
  lib.rs
  error.rs
  session.rs
  anthropic.rs
  tools.rs
  agent.rs
  repl.rs
```

Why this shape:

- `main.rs` is the binary entry point: install the Tokio runtime, call `cawir::run()`, and stay tiny.
- `lib.rs` is the crate root and public API: declare modules and re-export the small surface used by the binary.
- `repl.rs` is the current Surface implementation: startup setup, line-based stdin/stdout, slash commands, and rendering.
- `agent.rs` is Core engine orchestration: one user turn, model calls, tool calls, history mutation, and loop limits.

The reason for this split is transport independence. `repl.rs` is only one way to talk to the agent. Future TUI, stdio, WebSocket, or one-shot CLI surfaces should reuse `agent.rs`, `session.rs`, `tools.rs`, and provider code instead of duplicating the agent loop.

*Deferred:*
- `Provider` trait → CP6, when OpenAI is added.
- `Tool` trait + registry → CP6-7, when provider/tool registration pressure is real.
- `Command` trait + registry → CP6-7, when `/provider <name>` adds arguments and hook-configured commands add a dynamic source.
- `PermissionMode` enum + `/mode` → CP4.

Done when: `lib.rs` is mostly module declarations and public exports; behavior is unchanged; tests live beside the modules they exercise; `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings` pass after each sub-step.

Components: Core engine, Surface, External.

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

MCP tools · plugin loading · subagents · auto-mode classifier · context compaction · memory extraction.

### Future surfaces and transports

The current `repl.rs` is a plain line-based REPL, not a full terminal UI. Future Surface options:

- **TUI** — rich terminal interface with panes, scrolling, status, and keybindings. Likely Ratatui + Crossterm. This should be a Surface-layer swap: the TUI consumes agent events and submits prompts, while the agent loop stays underneath.
- **One-shot CLI** — `cawir exec "fix the tests"` style command. Arguments in, stdout/stderr out, process exits.
- **Stdio protocol mode** — long-running headless process that reads structured messages from stdin and writes structured events/results to stdout. Useful for editor integrations, scripts, or MCP/LSP-style supervision.
- **WebSocket server** — long-running network transport for a UI, daemon, or remote client. WebSocket is a transport: a persistent two-way pipe.
- **JSON-RPC protocol** — optional message protocol for request/response methods and errors. JSON-RPC is a different layer from WebSocket: it can run over stdio, WebSocket, or HTTP.

Each has a seam in [`architecture.md`](architecture.md). The principle is the same for all of them: Surface code changes, `agent.rs` and the core session/tool/provider logic should not.
