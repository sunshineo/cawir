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

## Three phases, fourteen checkpoints plus one hardening checkpoint

| Phase | Checkpoints |
|---|---|
| Foundation — not yet an agent | 1. Echo · 2. Chat |
| The agent | 3. Agent loop ⭐ · 3.5. Refactor shape |
| Craft and extension seams | 4. Providers/auth · 5. Modes · 6. Registries · 7. Resume · 8. Events · 8.5. Foundation hardening · 9. Hooks/settings · 10. Streaming · 11. MCP · 12. Plugins · 13. Skills · 14. Surfaces |

---

## 1 — Echo

Minimal REPL with `/exit` and `/help`.

- **1a — First read**. Prompt, read one line, echo, exit. *Rust:* `stdin`, `String`, `Result`, `?`, `print!`+`flush`.
    - *Iterates into:* `print!`/`read_line` is a Surface-layer-only pattern. It swaps out for [Ratatui](https://ratatui.rs) + Crossterm widgets if/when we take the TUI-upgrade seam at CP14. The agent loop underneath is unaffected.
- **1b — Loop forever**. Wrap 1a in `loop`; exit on EOF. *Rust:* `loop`, `break`, EOF detection.
    - *Iterates into:* The `loop { read stdin → dispatch }` shape stays largely unchanged through Chat (CP2) and Agent loop (CP3). At Agent events (CP8) the REPL becomes a consumer of structured `AgentEvent`s and the loop shifts toward event-driven rendering. The outer stdin loop survives above that as a "read user input" pump.
- **1c — Slash commands**. `/exit`, `/help`, else echo. *Rust:* `match` on `&str`, `trim`, `starts_with`.
    - *Iterates into:* The hardcoded `match` extracts into a `Command` trait + `CommandRegistry` at CP6, after `/provider <name>` and `/mode <name>` create concrete argument-parsing pressure. Plugin-loaded commands come later at CP12-13. Each current `match` arm maps one-to-one to a future registry entry — the refactor is a lift, not a rewrite.

*Deferred:* `Command` trait + `CommandRegistry` → CP6. Plugin-loaded commands → CP12-13. Write the 1c match so each arm is one refactor from a registry lookup.

Components: Surface.

---

## 2 — Chat

Multi-turn conversation with Claude. Biggest Rust jump — seven sub-steps.

- **2a-i — Async entry point**. Add `tokio`; make main `#[tokio::main] async fn`. Behavior unchanged — same prompt, same loop, same exit. *Rust:* adding a crate with feature flags, `#[tokio::main]` proc macro, `async fn` syntax (no `.await` yet).
- **2a-ii — First HTTP call**. Add `reqwest`; fetch a plain-text endpoint (e.g. `api.github.com/zen`) and print it before the REPL loop. Change main's return to `Result<(), Box<dyn Error>>` so `?` can propagate both `io::Error` and `reqwest::Error`. Used `reqwest::Client::builder()` (rather than the simpler `reqwest::get`) so we could set a `User-Agent` header — GitHub's API requires it. *Rust:* the builder pattern, `reqwest::Client`, `.send().await? → .text().await?` chained, `Box<dyn Error>` as a catch-all type-erased error, automatic error conversion via the `From` trait.
    - *Iterates into:* The GitHub Zen demo call gets removed when the first Claude call lands at 2c — replaced, not extended. The `reqwest::Client::builder()` pattern stays (we'll set `Authorization` and other headers the same way for Anthropic). `Box<dyn Error>` persists as a pragmatic stepping stone until 2f, where a `thiserror` enum takes over.
- **2b — Parse JSON**. Fetch `https://api.github.com/repos/rust-lang/rust`, deserialize into a `Repo` struct via `#[derive(Deserialize)]`, print selected fields (name, description, stars, issues, forks). *Rust:* `#[derive(Deserialize)]` with `serde`, `reqwest::Response::json().await?`, `Option<T>` for nullable JSON fields.
    - *Iterates into:* The GitHub repo demo is replaced at 2c when Claude API request/response types take over. The `#[derive(Deserialize)]` pattern persists — reused in every checkpoint from 2c onward, and again at CP10 for stream events (smaller enum variants tagged by a `type` field, one per SSE event, instead of one big response struct).
- **2c — First Claude call**. Hard-coded "hello" POST. cawir reads `ANTHROPIC_API_KEY` from env, builds a `MessageRequest` with the Claude model + a single user `Message`, sends it with `x-api-key` / `anthropic-version` / `content-type` headers, parses the response into `MessageResponse` with `Vec<ContentBlock>`, prints the first block's text. Status check before parsing so 401s give a clear error message instead of a confusing serde failure. *Rust:* `#[derive(Serialize)]`, custom HTTP headers, `std::env::var`, `if let Some(...)`, status-then-body error handling.
    - *Iterates into:* The hard-coded "hello" prompt is replaced by user input at **2d (Wire with REPL)**, where each line becomes a Claude call and `?`-everywhere fail-fast becomes per-call match. Single-shot becomes multi-turn at **2e** with `Vec<Message>` accumulating across turns. The request grows a first concrete tool at **3a (First tool advertised)**, and the simple `ContentBlock { text: String }` grows into a `#[serde(tag = "type")]` enum at **3b (Parse tool-use responses)**. `Box<dyn Error>` becomes a `thiserror` enum at **2f**.
- **2d — Wire with REPL**. Replace the hard-coded "hello" with user input from the REPL. Each non-`/command` line goes to Claude as a one-shot prompt; the reply prints; no history yet. Extract the Claude call into `async fn ask_claude(&Client, &str, &str) -> Result<String, _>`. The call site uses `match` instead of `?` — a network blip or 401 prints to stderr but the REPL keeps running. *Rust:* function extraction with `&` parameters, `async fn` returning `Result`, the fail-fast → graceful transition for runtime errors (setup-time still uses `?`), `eprintln!` for stderr.
    - *Iterates into:* `ask_claude` becomes a method on the `Provider` trait at **CP4 (Providers/auth)** once we have a second concrete impl (OpenAI) to extract from. The single-turn no-history behavior becomes multi-turn at **2e** with `Vec<Message>` accumulating across loop iterations. Hardcoded model name and `max_tokens` move to config when provider/settings pressure arrives.
- **2e — Multi-turn**. `history: Vec<Message>` accumulates across loop iterations; the full history ships in every API call so Claude has context. Push the user message before the call, and pop it if the call fails (Anthropic rejects two consecutive user turns, so history must stay clean on errors). *Rust:* `Vec<T>` mutation patterns (`push`, `pop`), `Vec::new()` vs `vec![]`, slice parameters (`&[Message]` instead of `&Vec<Message>`), `Clone` derive, `.to_vec()` to clone a slice into an owned `Vec`.
    - *Iterates into:* The history is in-memory only; persists across `/exit` + restart at **CP7 (Resume)** by serializing session data to disk. `Message`'s `content: String` gets enriched at **3e (Send one tool result back)** — once assistant tool-use blocks and tool results land, content can't flatten to a string anymore. The unbounded growth of `history` will eventually need compaction (currently a "Beyond CP14" speculative seam in the architecture).
- **2f — Cleanup**. Split the one-file prototype into `main.rs` + `lib.rs` + `session.rs` + `error.rs`; add `thiserror`; replace `Box<dyn Error>` with a typed app error enum and project `Result<T>` alias. Add `AGENTS.md -> CLAUDE.md` so Codex reads the same project guidance. *Rust:* binary vs library crates, `mod`, `pub`, `pub use`, type aliases, `#[derive(thiserror::Error)]`, `#[error(...)]`, `#[from]`, `std::convert::From`, how `?` converts errors, the Rust prelude (`Debug`, `From`).
    - *Iterates into:* `Message` now lives in `session.rs` as pure conversation data, ready to grow toward CP7 persistence. `ask_claude` remains a concrete Anthropic function; it becomes a provider method only after a second provider creates Rule-of-Three pressure at **CP4 (Providers/auth)**. The error enum can now gain variants as CP3 introduces file IO, shell execution, tool dispatch, and permission failures.

Components: Surface, Core engine (minimal), External (hard-coded Anthropic call, no `Provider` trait yet).

---

## 3 — Agent loop ⭐

The soul of cawir. cawir stops being "chat with Claude" and becomes a real coding agent: first by advertising one concrete tool, then by handling `tool_use`, then by executing approved local actions, and only then by looping until Claude stops.

- **3a — First tool advertised**. Add a single concrete tool, `read_file`, to the Anthropic request so Claude can reach for a real tool instead of only answering in text. This step is allowed to expose the next failure mode: if Claude emits `tool_use`, the old parser may break, and **3b** fixes that. *Rust:* extending request structs, `serde_json::Value`, the `json!` macro, hand-built JSON schema data.
    - *Iterates into:* The first tool stays concrete and inline. Its schema is allowed to be raw `serde_json::Value` at first because that mirrors the provider docs directly. If several tools make that too repetitive or too typo-prone, extract small typed schema helpers from the concrete repetition. The tool definition extracts into a `Tool` trait + registry at **CP6**, once there is concrete pressure from multiple tools and future external tool sources.
- **3b — Parse tool-use responses**. Replace the text-only `ContentBlock` parsing with a tagged enum that handles both `text` and `tool_use`. cawir can now receive a tool request without deserialization failure. *Rust:* `#[serde(tag = "type")]`, data-carrying enums, `match` on enum variants.
    - *Iterates into:* The same tagged-enum deserialization pattern comes back at **CP10 (Streaming)**, where SSE events also arrive as smaller typed variants keyed by a `type` field.
- **3c — Execute one read-only tool call**. Match on a parsed `read_file` tool call, extract its `path`, run the local file read, and surface the raw result. *Rust:* `serde_json::Value`, simple input extraction, `std::fs::read_to_string`, `match` dispatch.
    - *Iterates into:* The plain `match` dispatcher is deliberate. Each arm should already look like the future trait method signature, but no `Tool` trait or registry is extracted yet.
- **3d — Second read-only tool: list_files**. Add `list_files` so Claude can inspect repository or folder structure before choosing files to read. Match on a parsed `list_files` tool call, extract its `path`, run a directory listing, and surface the raw result. *Rust:* `std::fs::read_dir`, `DirEntry`, collecting and formatting owned `String` output.
    - *Iterates into:* This stays concrete and inline beside `read_file`. Together they make the read-only inspection path more useful before any approval system exists, while still keeping dispatch as a plain `match`.
- **3e — Send one tool result back**. Enrich session or message data enough to store assistant tool-use content and a `tool_result`, then send the `read_file` or `list_files` result back and print Claude's follow-up answer. *Rust:* richer serde enums or structs for conversation state, owned `Vec` data, cloning and ownership across request assembly.
    - *Iterates into:* This is the moment `Message` stops flattening to plain text. The same "session is pure serde data" discipline later pays off at **CP7 (Resume)**.
- **3f — Repeat until Claude stops**. Turn the one-shot call-execute-feedback flow into the real read-only agent loop: keep executing `read_file` and `list_files` calls until Claude returns a normal stop. *Rust:* loop control, stop conditions, repeated request or response cycles.
    - *Iterates into:* The control-flow shape here is the foundation for later event emission and streaming. CP8 and CP10 change how the loop surfaces work, not the fact that the loop owns orchestration.
- **3g — Tool failures become tool results**. Missing files and invalid tool inputs are returned to Claude as tool errors instead of crashing or corrupting session history. *Rust:* error-to-data conversion, keeping state consistent after partial failure.
    - *Iterates into:* CP5 adds policy errors with structure; CP9 adds hook-driven denial. CP3 only needs the simpler rule that a tool failure should stay inside the conversation, not tear down the session.
- **3h — First mutating tool: write_file with inline approval**. Add `write_file`, but gate it with a direct REPL approval prompt before execution. Denials and write failures flow back through the tool-result path from 3g. *Rust:* `std::fs::write`, inline approval flow, distinguishing denial from execution failure.
    - *Iterates into:* The approval check is intentionally hardcoded here so mutating tools can land safely before the real policy system. `PermissionMode` starts at **CP5 (Modes)**.
- **3i — Second mutating tool: shell with inline approval**. Add `shell`, also behind direct approval, and return stdout, stderr, denial, or execution failure into the tool-result path. *Rust:* `std::process::Command`, exit-status handling, stdout or stderr capture.
    - *Iterates into:* Shell approval remains a local REPL concern in CP3. Finer-grained policy and hook-based vetoes still wait for **CP5** and **CP9**.

*Deferred:*
- `Tool` trait + `ToolRegistry` → CP6
- `PermissionMode` enum + `/mode` → CP5
- Plan mode + `ExitPlanMode` → CP5
- Multi-source tool registration → CP11-13
- Tool-output budgeting → CP8.5 foundation hardening. Add file-size caps or ranged reads so one `read_file` cannot inject a huge file into history by default. Add stdout/stderr byte or line caps for `shell`, with clear truncation markers; Checkpoint 3i currently decodes process output into one lossy UTF-8 string for simplicity.
- Rate-limit recovery → CP8.5 foundation hardening. Parse 429 `retry-after` / rate-limit headers and decide whether to wait, retry, or return a clearer recoverable error.
- Cache observability → CP8.5 foundation hardening. Parse provider usage fields such as Anthropic `input_tokens`, `cache_creation_input_tokens`, and `cache_read_input_tokens` so cache behavior is visible while learning.
- Prompt-cache stability → CP8.5 foundation hardening. Keep tools, system/project instructions, and per-turn messages as separate request components so cache invalidation is deliberate and observable. Tool schemas still travel through provider-native structured tool fields, but because Anthropic includes tools in the cached prefix, tool order and definitions must remain stable unless the tool surface intentionally changes.

Write dispatch so each arm is already the future trait method's signature.

Done when: "read Cargo.toml and tell me the deps" works end-to-end; approved writes and shell commands work; one user prompt can trigger multiple tool calls; and tool failures stay inside the conversation instead of killing the session.

Components: Core engine, Surface.

---

## 3.5 — Refactor shape

Checkpoint 3 made cawir a real agent, but `lib.rs` now owns too many responsibilities: Anthropic HTTP, tool definitions, tool execution, approval prompts, the agent loop, slash commands, and most tests. Before adding permission modes, split the code by responsibility while preserving behavior.

This is an organizational checkpoint, not an abstraction checkpoint. The goal is clearer modules, not a provider trait, tool trait, command registry, plugin system, or config system.

- **3.5a — Move Anthropic API code to `anthropic.rs`**. Move request/response structs and the concrete `ask_claude` call out of `lib.rs`. Keep the implementation Anthropic-specific. *Rust:* `pub(crate)` visibility, module imports, separating wire protocol code from orchestration.
    - *Iterates into:* A future `Provider` trait still waits until CP4 when OpenAI adds real extraction pressure. This step only gives the concrete Anthropic code a home.
- **3.5b — Move tools to `tools.rs`**. Move tool schemas, tool dispatch, tool execution, approval prompts, and tool tests out of `lib.rs`. Add a small concrete `definitions() -> Vec<ToolDefinition>` and `execute_tool_uses(...)` surface, but keep dispatch as a plain `match`. *Rust:* private helpers inside a module, public module functions, tests living beside the code they exercise.
    - *Iterates into:* A `Tool` trait + registry still waits until CP6. This step makes the current match registry-shaped without introducing dynamic dispatch. `execute_tool_uses(...)` is allowed to be a slightly orchestration-heavy boundary for now because it keeps the 3.5b call site simple. When the agent loop and event handling grow, expect it to split: `tools.rs` keeps the low-level "execute one named tool with JSON input" responsibility, while `agent.rs` owns turning model `tool_use` blocks into events, tool results, and session updates.
- **3.5c — Move agent loop to `agent.rs`**. Move `run_agent_turn`, `MAX_TOOL_ROUNDS`, and loop orchestration out of `lib.rs`. The agent loop calls the concrete Anthropic module and the concrete tools module. *Rust:* borrowing `&mut Vec<Message>` through orchestration, module boundaries around control flow.
    - *Iterates into:* CP8 events, CP9 hooks, and CP10 streaming will change how the loop emits progress, but this step preserves the current request-tool-result loop.
- **3.5d — Move REPL and slash commands to `repl.rs`**. Move `run`, `print_help`, startup setup, and the slash-command `match` out of `lib.rs`. Leave `/exit` and `/help` concrete. *Rust:* library exports, binary crate calling `cawir::run`, keeping surface code separate from engine code.
    - *Iterates into:* `/provider` in CP4 and `/mode` in CP5 will extend the concrete slash-command match before any command registry exists. Later Surface implementations can sit beside or replace `repl.rs`: a richer TUI, a one-shot `exec` command, stdio protocol mode, or a WebSocket server.

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
- `Provider` trait → CP4, when OpenAI is added.
- `Tool` trait + registry → CP6, when registration pressure is real.
- `Command` trait + registry → CP6, after `/provider <name>` and `/mode <name>` add arguments.
- `PermissionMode` enum + `/mode` → CP5.

Done when: `lib.rs` is mostly module declarations and public exports; behavior is unchanged; tests live beside the modules they exercise; `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings` pass after each sub-step.

Components: Core engine, Surface, External.

---

## Remaining checkpoint order

The rest of the roadmap is ordered by dependency, not by feature excitement:

```text
4 Providers/auth
5 Modes
6 Registries
7 Resume
8 Agent events
8.5 Foundation hardening
9 Hooks/settings
10 Streaming
11 MCP
12 Plugins
13 Skills
14 Alternate surfaces
```

Reasoning:

- Provider/auth comes first because the code is still Anthropic-shaped; everything later should depend on a model boundary, not on `ask_claude`.
- Modes come before tool registries so the existing inline approval path becomes a real policy state machine.
- Registries come before MCP/plugins/skills because those are capability sources; they need somewhere to register tools, commands, and context.
- Resume comes before events/hooks/streaming because it forces a clean split between durable `Session` data and non-serializable runtime handles.
- Events come before hooks, streaming, and alternate surfaces because all three need the same lifecycle vocabulary.
- Foundation hardening comes before hooks/settings and external capability sources because tools, events, prompt assembly, provider behavior, and runtime ownership should be solid before third-party code can extend them.

---

## 4 — Providers, Auth, Config

Move from hard-coded Anthropic to selectable providers with explicit credential handling.

Sub-steps:

- **4a — Add OpenAI and extract `Provider`**. Keep Anthropic concrete, add OpenAI concrete, then extract the shared provider shape from the two implementations. *Rust:* traits extracted from real duplication, associated data, static vs dynamic dispatch choices.
- **4b — `/provider <name>`**. Add the first slash command with an argument using the existing concrete match. Bare `/provider` lists the current provider and available provider names; `/provider <name>` switches the active provider. Switching providers does not clear conversation history, so the next request sends the existing session to the new provider and should warn the user about that. No interactive picker and no command registry yet. *Rust:* parsing command arguments with `split_whitespace`, returning user-facing errors without panicking.
- **4c — Credential options and credential chain**. Split wire format from credential attachment. Implement `ApiKey` and `CodexOAuth` for OpenAI, lookup order credentials file → environment → `.env`, credential acquisition for API keys and Codex OAuth, and saving the selected provider + credential option for the next launch. *Rust:* enums for auth options, owned secret data, serde config files, file permissions, crate choice for `directories`, `rpassword`, and `base64`.
- **4d — Add Ollama**. Local no-auth provider to pressure-test `Provider` + `AuthMethod` without another cloud credential. *Rust:* provider-specific request/response structs behind one trait.
- **4e — Provider config cleanup**. After Ollama lands, revisit startup selection across Anthropic, OpenAI, and Ollama. First launch chooses the first provider with usable credentials when no saved preference exists; saved provider + credential option from 4c remains the normal path.

Deferred provider-boundary cleanup:

- `ActiveProvider` currently uses explicit enum delegation instead of `Box<dyn Provider>` so the provider set stays concrete and visible while learning. This avoids heap allocation and vtable dispatch, but every new `Provider` method and provider variant adds forwarding boilerplate. If the provider list grows beyond Anthropic/OpenAI/Ollama or the `Provider` trait gains enough methods that manual delegation becomes noisy, evaluate an enum-delegation crate such as `enum_dispatch` or `enum_delegate`. These crates generate the same enum `match` forwarding code while preserving the static enum shape. Do not add one preemptively; first split an overgrown `Provider` trait if the trait itself is carrying too many responsibilities.

Done when: Anthropic, OpenAI, and Ollama can be selected; credential lookup is explicit; `agent.rs` calls a provider boundary instead of `ask_claude`; `/provider anthropic|openai|ollama` works.

Components: External, Surface.

---

## 5 — Modes And Plan Mode

Replace inline approval-only behavior with explicit permission modes.

Sub-steps:

- **5a — `PermissionMode` enum**. Add `Default`, `Plan`, `AcceptEdits`, and `Bypass`. Start in `Default`. *Rust:* enum as state machine, exhaustive `match`.
- **5b — `/mode <name>`**. Add mode switching in the concrete slash-command match. No command registry yet.
- **5c — Mode-aware tools**. Thread the current mode into tool execution. `Default` asks on mutating tools, `AcceptEdits` auto-approves writes but still asks on shell, `Bypass` allows everything.
- **5d — Plan mode**. In `Plan`, mutating tools are denied as tool results. Add a concrete `exit_plan_mode` tool that returns a proposed plan upward for REPL approval before switching modes.

Done when: `/mode default`, `/mode plan`, `/mode accept-edits`, and `/mode bypass` work; plan mode blocks mutation; `exit_plan_mode` produces an approval prompt; approved plans can continue.

Components: Policy, Surface, Core engine.

---

## 6 — Tool And Slash-Command Registries

Create the registration points needed before MCP, plugins, skills, and more slash commands.

Sub-steps:

- **6a — `Tool` trait and `ToolRegistry`**. Move the current tool `match` into named concrete tool implementations registered at startup. *Rust:* trait objects, `Box` or `Arc`, object safety.
- **6b — Tool metadata**. Keep tool name, description, input schema, mutating/read-only classification, and execution in one place.
- **6c — `Command` trait and `CommandRegistry`**. Move `/exit`, `/help`, `/provider`, and `/mode` into a simple registry after they have real argument pressure.
- **6d — Keep built-ins concrete**. Built-in tools and commands still register directly in Rust code. External population waits for MCP/plugins/skills.

Done when: adding a built-in tool or slash command no longer requires editing a central dispatch `match`; behavior is unchanged.

Components: Capabilities, Surface.

---

## 7 — Sessions And Resume

Persist conversations and make runtime state separate from durable session data.

Session-design note:

- The internal session format should be designed for cawir's own needs: durable conversation state, context management, resume, compaction, tool-result history, provider/mode metadata, and future agent quality work. It should not be treated as a mirror of any provider's API shape. Anthropic and OpenAI already prove that provider adapters can translate between one internal `Message`/content model and different wire formats. If the internal model changes in CP7, prefer a shape that helps the agent manage context well, then make each provider adapter map that shape to its API.

Sub-steps:

- **7a — `Session` struct**. Wrap message history in a serializable `Session { id, messages, provider, mode, ... }`.
- **7b — Runtime struct**. Move non-serializable handles such as HTTP client, provider registry, tool registry, and command registry into `Runtime`.
- **7c — Session storage**. Use `directories` to find an OS-appropriate data path. Save sessions as JSON.
- **7d — CLI args**. Add `clap` for `cawir --resume <id>` and `cawir --continue`.

Done when: a conversation survives `/exit` + restart; `--continue` opens the most recent session; `--resume <id>` opens a specific session.

Components: Core engine, Surface, External.

---

## 8 — Agent Events

Introduce a typed event vocabulary between the agent loop and whatever is rendering or observing it.

Sub-steps:

- **8a — `AgentEvent` enum**. Start with `UserPromptSubmit`, `ModelRequestStart`, `ToolUseRequested`, `ToolUseFinished`, `AssistantText`, `Stop`, and `StopFailure`.
- **8b — Emit events from `agent.rs`**. Preserve current terminal output at first by rendering events in `repl.rs`.
- **8c — Event-aware tool results**. Keep tool results as session data, but make progress and display a separate event stream.

Done when: `repl.rs` renders agent progress from typed events instead of ad hoc `println!` calls inside the core loop.

Components: Core engine, Surface.

---

## 8.5 — Foundation Hardening

Before adding hooks, MCP, plugins, skills, or alternate surfaces, tighten the foundation that external capabilities will depend on. This is not a feature-expansion checkpoint. It is a cleanup checkpoint for the places where checkpoints 1-8 deliberately stayed simple, plus the places where implementation grew beyond the roadmap and needs to be made explicit.

Sub-steps:

- **8.5a — Workspace path policy**. Enforce the "current project" boundary that tool descriptions already imply. Normalize and validate paths for `read_file`, `list_files`, and `write_file`; decide how explicit user requests for outside-project paths are represented; return denied path attempts as tool errors rather than panics. *Rust:* `Path`, `PathBuf`, `canonicalize`, prefix checks, path traversal tests.
- **8.5b — Tool output budgets and process limits**. Add bounded output for `read_file`, `list_files`, and `shell`: file-size caps or ranged reads, directory-entry caps, stdout/stderr byte caps, clear truncation markers, and a shell timeout. Keep truncation visible to the model so it can ask for narrower reads. *Rust:* byte vs char boundaries, lossy UTF-8, `Command` timeout patterns, helper structs for budgeted output.
- **8.5c — Patch-style editing tool**. Add a safer editing primitive beside `write_file`, such as `edit_file` or `apply_patch`, so routine code changes do not require replacing entire files. Keep `write_file` for new files and complete rewrites. *Rust:* string searching vs structured patch data, error variants for ambiguous matches, tests around exact replacements.
- **8.5d — Prompt assembly and project memory**. Introduce a concrete prompt assembly layer that builds provider-neutral instructions from named sections: identity, behavior, environment, project guidance, and later active skills. Load `AGENTS.md` / `CLAUDE.md` from the project hierarchy before the model call, without inlining tool schemas into prompt text. Keep prompt sections deterministic: avoid per-request timestamps or other volatile text inside cacheable instruction/project-memory sections unless the volatility is intentional. *Rust:* owned prompt data, filesystem lookup order, separating durable session messages from per-request prompt context.
- **8.5e — Runtime-owned registries**. Move tool registry ownership into `Runtime` instead of rebuilding built-ins inside `tools.rs`, and make the agent loop receive registry references explicitly. Keep built-ins concrete, but make the ownership shape ready for CP9 hooks, CP11 MCP, and CP12 plugins. Preserve deterministic tool definition ordering because providers such as Anthropic include structured tools in the prompt-cache prefix; mode-specific tool availability should be explicit and auditable. *Rust:* borrowing runtime state across async calls, trait object lifetimes, `&ToolRegistry` vs `Arc<ToolRegistry>`.
- **8.5f — Provider robustness and observability**. Add provider-facing recovery and diagnostics that the current `Provider` boundary needs before more features depend on it: parse 429 `retry-after` where providers expose it, return clearer recoverable rate-limit errors, expose token usage where available, and surface Anthropic cache creation/read counts. Make cache hits and misses visible enough to catch silent non-caching caused by short prompts, unstable prefixes, tool changes, or provider request-shape mistakes. *Rust:* optional response fields, typed provider metadata, preserving provider-neutral errors.
- **8.5g — Event boundary hardening**. Expand events enough for hooks and alternate surfaces: session start/end, pre-tool and post-tool events, model request finish, and structured stop/failure metadata. Decide whether the current callback stays for now or becomes a lightweight stream-like adapter. Keep `tool_result` blocks as session data, not display events. *Rust:* serializable event enums, producer/consumer boundaries, callback vs stream tradeoffs.
- **8.5h — Anthropic prompt-cache request audit**. Verify the current Anthropic `cache_control` request shape against Anthropic's documented automatic and block-level prompt caching formats. Confirm whether top-level automatic caching is sufficient for cawir's agent loop, or whether explicit breakpoints should mark stable tools/system/project-memory separately from growing conversation history. Audit against the Claude Code April 2026 postmortem failure mode: cawir should not send thinking-clearing headers or otherwise mutate prior reasoning/context in a way that causes repeated cache misses. *Rust:* provider-specific serde structs, `skip_serializing_if`, tests that assert the exact wire JSON.
- **8.5i — Hook-readiness root and payload hardening**. Make prompt assembly, tool workspace policy, and future hook execution agree on the same session project root instead of letting tools derive a separate root from process cwd. Enrich `PostToolUse` with the original tool input so command hooks can make stateless decisions, such as formatting a written `.rs` file, from the event JSON they receive. *Rust:* borrowing `&Path` through nested calls, serializable event payloads, tests for resumed-session root mismatches and post-tool hook data.

Prompt-cache follow-up: 8.5h leaves cawir with one explicit Anthropic breakpoint on the assembled system prompt plus top-level automatic caching for the growing conversation. Do not add more explicit breakpoints just because Anthropic supports them. Add another breakpoint only when a later checkpoint introduces a request layer with a different stability profile, such as compaction summaries, memory extraction, subagent handoffs, dynamic MCP/plugin/skill context, or large generated tool surfaces. That future step should name the stable layer, update the provider-neutral request shape if needed, and assert the exact Anthropic wire JSON.

Done when: tools enforce project boundaries from the same project root used by prompt assembly, large outputs and long processes are bounded, routine edits can happen without whole-file rewrites, prompt assembly has a real module with stable cacheable sections, runtime owns registries with deterministic tool definitions, provider errors/usage are more observable, events are ready for hooks with enough payload for stateless command handlers, and Anthropic cache behavior is both correct and visible.

Components: Core engine, Capabilities, Policy, External, Surface.

---

## 9 — Hooks And Settings

Let configured handlers observe, modify, or deny work at event points.

Sub-steps:

- **9a — `SettingsResolver`**. Load settings from user, project, and local files in a deterministic merge order.
- **9b — `HookRegistry`**. Register handlers by event kind.
- **9c — Command hooks**. Run configured commands with event JSON on stdin; parse allow/deny/modify actions from exit status and stdout.
- **9d — Pre-tool denial**. Demonstrate a hook denying a tool before execution.
- **9e — Post-write hook demo**. Example: run `cargo fmt` after `write_file` on `.rs`.

Done when: a configured hook fires; `PreToolUse::Deny` blocks a tool; a post-write hook can format Rust files.

Components: Core engine, Capabilities, External.

---

## 10 — Streaming

Stream provider output and tool-use deltas through the event system.

Sub-steps:

- **10a — Anthropic SSE**. Parse streaming events into typed provider events.
- **10b — OpenAI streaming**. Map OpenAI stream chunks into the same provider-neutral event shape.
- **10c — Agent integration**. Preserve tool calls and tool results while text streams.
- **10d — REPL rendering**. Render partial assistant text without corrupting approval prompts or tool progress.

Done when: assistant text appears token-by-token; tool calls still work during streamed turns.

Components: External, Core engine, Surface.

---

## 11 — MCP Tools

Add MCP as an external source of tools.

Sub-steps:

- **11a — MCP client process management**. Start and stop configured MCP servers.
- **11b — Tool discovery**. Convert MCP tool metadata into `ToolRegistry` entries.
- **11c — Tool invocation**. Dispatch model-requested MCP tools through the same permission and event path as built-ins.

Done when: a configured MCP server contributes at least one callable tool and tool results flow through the normal agent loop.

Components: Capabilities, External.

---

## 12 — Plugins

Add local plugin packages as a structured source of commands, tools, hooks, and settings.

Sub-steps:

- **12a — Plugin manifest**. Define a minimal plugin metadata file.
- **12b — Plugin discovery**. Load plugins from configured directories.
- **12c — Plugin contributions**. Allow plugins to register built-in-style commands, external command tools, hooks, and settings snippets.

Done when: a local plugin can add one slash command and one external command-backed tool without code changes in core modules.

Components: Capabilities, External, Surface.

---

## 13 — Skills

Add reusable instruction/context bundles that can teach the agent specialized workflows.

Sub-steps:

- **13a — Skill format**. Define a small local skill format with name, description, trigger guidance, and instruction body.
- **13b — Skill loading**. Load skills from configured directories and plugin-provided skill folders.
- **13c — Skill activation**. Add selected skill instructions to prompt assembly when the user names a skill or trigger guidance matches.

Done when: a skill can add durable workflow guidance without changing Rust code, and active skills are visible in prompt assembly/debug output.

Components: Capabilities, Core engine, External.

---

## 14 — Alternate Surfaces

Add new ways to talk to the same agent engine.

Sub-steps:

- **14a — One-shot CLI**. `cawir exec "..."` runs one prompt headlessly and exits.
- **14b — Stdio protocol mode**. Long-running process reads structured messages from stdin and writes structured events/results to stdout.
- **14c — TUI**. Rich terminal UI with panes, scrolling, status, and keybindings. Likely Ratatui + Crossterm.
- **14d — WebSocket / JSON-RPC server**. WebSocket is the transport; JSON-RPC is an optional request/response protocol that can run over WebSocket, stdio, or HTTP.

Done when: at least one non-REPL surface can drive the same `agent.rs` core without duplicating the agent loop.

Components: Surface.

---

## Rust concepts by checkpoint

| # | Checkpoint | New Rust concepts |
|---|---|---|
| 1 | Echo | `String`/`&str`, `stdin`, `Result`, `?`, `loop`, `match` |
| 2 | Chat | crates, `async`/`await`, serde derive, `Vec`, structs/enums, modules, `thiserror`, env vars |
| 3 | Agent loop | agent-loop protocol, `serde_json::Value`, `std::fs`, `Command`, match dispatch |
| 4 | Providers/auth | traits from real duplication, trait composition, config paths |
| 5 | Modes | enum state machine, exhaustive match, policy as data |
| 6 | Registries | trait objects, object safety, `Box`/`Arc` |
| 7 | Resume | serde disk I/O, `clap`, `directories`, runtime vs durable data |
| 8 | Agent events | event enums, producer/consumer boundaries |
| 8.5 | Foundation hardening | path normalization, output budgeting, runtime ownership, wire-format audits |
| 9 | Hooks/settings | settings merge, process IO, sync decision points |
| 10 | Streaming | `futures::Stream`, SSE parsing, partial JSON |
| 11 | MCP | external protocol clients, process lifecycle |
| 12 | Plugins | manifest parsing, contribution registration |
| 13 | Skills | prompt assembly, dynamic context selection |
| 14 | Surfaces | transport boundaries, stdio/WebSocket protocol layering |

## Beyond Checkpoint 14 (speculative)

Subagents · auto-mode classifier · context compaction · memory extraction · cache-breakpoint strategy expansion for new context layers · richer TUI polish · remote daemon deployment.

Each has a seam in [`architecture.md`](architecture.md). The principle is the same for all of them: extend the relevant component boundary only after concrete pressure exists.
