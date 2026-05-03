# cawir status

This document tracks the current implementation state and recent progress.

- Stable collaboration rules live in [`../AGENTS.md`](../AGENTS.md) / [`../CLAUDE.md`](../CLAUDE.md).
- Checkpoint sequence and scope live in [`roadmap.md`](roadmap.md).
- Target design and extension seams live in [`architecture.md`](architecture.md).

## Snapshot (as of 2026-05-03)

- Current focus: **Checkpoint 4 — Providers, Auth, Config**.
- Next checkpoint: **4 — Providers, Auth, Config**.
- Latest completed sub-step: **3.5d — Move REPL and slash commands to `repl.rs`**.
- Planned next order: add OpenAI and extract a concrete `Provider` boundary, then add provider selection, auth methods, credential lookup, Ollama, and provider config before moving to modes.
- Current user-visible behavior: one user prompt can trigger repeated `read_file`, `list_files`, approval-gated `write_file`, and approval-gated `shell` calls. cawir executes each tool request, sends matching `tool_result` blocks back to Claude, and continues until Claude returns a text answer or the 42-round tool-loop cap is reached. Tool execution failures and denied mutating actions are returned to Claude as error `tool_result` blocks instead of aborting the turn.

## Completed checkpoints

### 1 — Echo

- `1a`, `1b`, and `1c` completed on 2026-04-23. The REPL loops, handles `/exit` and `/help`, rejects unknown slash commands, and echoes all other input.

### 2 — Chat

- `2a-i` completed on 2026-04-24. Added `tokio`; `main` is now `#[tokio::main] async fn`.
- `2a-ii` completed on 2026-04-24. Added `reqwest`; first `.await` on a network call; temporary `Box<dyn Error>` return type.
- `2b` completed on 2026-04-24. Added `serde`; first JSON deserialization into a Rust struct.
- `2c` completed on 2026-04-25. First Anthropic call using `ANTHROPIC_API_KEY`.
- `2d` completed on 2026-04-27. The REPL sends each non-command line to Claude and keeps running after per-call failures.
- `2e` completed on 2026-04-27. `history: Vec<Message>` makes the conversation multi-turn.
- `2f` completed on 2026-04-28. Split code into `main.rs`, `lib.rs`, `session.rs`, and `error.rs`; added `thiserror`; replaced `Box<dyn Error>` with `cawir::Result<T>`; `AGENTS.md` now symlinks to `CLAUDE.md`.

### 3 — Agent loop

- `3a` completed on 2026-04-29. `read_file` is advertised in the Anthropic request.
- `3b` completed on 2026-04-29. `ContentBlock` is now a tagged enum that can parse `tool_use`.
- `3c` completed on 2026-04-30. `read_file` executes and prints raw output through the local tool path.
- `3d` completed on 2026-04-30. `list_files` is advertised and executes as a second read-only tool, but the REPL still stops until `3e` sends a `tool_result` back to Claude.
- `3e` completed on 2026-04-30. `Message` content is now stored as serializable content blocks, so cawir can append an assistant `tool_use`, send one user `tool_result`, and print Claude's follow-up response.
- `3f` completed on 2026-04-30. The one-shot tool-result path is now a read-only agent loop: cawir keeps calling Claude, executing `read_file` and `list_files`, and appending tool results until Claude returns text.
- `3g` completed on 2026-05-01. Tool execution failures now become `tool_result` blocks with `is_error: true`, so Claude can recover from missing files, invalid tool input, and unknown tool names inside the loop.
- `3h` completed on 2026-05-01. Added `write_file` as the first mutating tool, gated by an inline REPL approval prompt. Approved writes use `std::fs::write`; denied writes and write failures flow back through error `tool_result` blocks.
- `3i` completed on 2026-05-01. Added `shell` as the second mutating tool, gated by an inline REPL approval prompt. Approved commands run through `std::process::Command`, and stdout, stderr, exit status, denials, and execution failures flow back through the tool-result path.

### 3.5 — Refactor shape

- `3.5a` completed on 2026-05-01. Moved concrete Anthropic request/response structs, `ask_claude`, text rendering, and Anthropic API tests into `src/anthropic.rs`. `lib.rs` still owns the agent loop, REPL, and tools until later 3.5 sub-steps.
- `3.5b` completed on 2026-05-01. Moved tool schemas, tool dispatch, tool execution, approval prompts, and tool tests into `src/tools.rs`. `lib.rs` now uses the concrete `tools::definitions()` and `tools::execute_tool_uses()` surface while the tool internals stay private.
- `3.5c` completed on 2026-05-02. Moved `MAX_TOOL_ROUNDS`, `run_turn`, and the request-tool-result orchestration into `src/agent.rs`. `lib.rs` now keeps REPL/startup concerns and calls the concrete agent module for each non-command user turn.
- `3.5d` completed on 2026-05-03. Moved `run`, startup setup, slash-command handling, and REPL rendering into `src/repl.rs`; `lib.rs` is now module declarations plus public exports. Moved remaining session serialization tests beside `Message` in `src/session.rs`.

## Learnings

- `learnings-rust/` currently includes notes `01` through `27`.
- `learnings-agent/` currently includes notes `01` through `04`.
- New Rust discussions should be distilled into `learnings-rust/*.md`; new agent-design discussions should be distilled into `learnings-agent/*.md`.
