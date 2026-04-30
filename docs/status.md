# cawir status

This document tracks the current implementation state and recent progress.

- Stable collaboration rules live in [`../AGENTS.md`](../AGENTS.md) / [`../CLAUDE.md`](../CLAUDE.md).
- Checkpoint sequence and scope live in [`roadmap.md`](roadmap.md).
- Target design and extension seams live in [`architecture.md`](architecture.md).

## Snapshot (as of 2026-04-30)

- Current focus: **Checkpoint 3 — Agent loop**.
- Next sub-step: **3d — Send one tool result back**.
- Latest completed sub-step: **3c — Execute one read-only tool call**.
- Current user-visible behavior: if Claude emits `read_file`, cawir prints the raw file contents and then stops the REPL. The turn cannot continue until `3d` sends a `tool_result` back to Claude.

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
- `3c` completed on 2026-04-30. `read_file` executes, prints raw output, and stops the REPL until `3d` exists.

## Learnings

- `learnings/` currently includes notes `01` through `20`.
- New Rust discussions should be distilled into `learnings/*.md` before commit, not left only in chat history.
