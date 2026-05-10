# cawir status

This document tracks the current implementation state and recent progress.

- Stable collaboration rules live in [`../AGENTS.md`](../AGENTS.md) / [`../CLAUDE.md`](../CLAUDE.md).
- Checkpoint sequence and scope live in [`roadmap.md`](roadmap.md).
- Target design and extension seams live in [`architecture.md`](architecture.md).
- Test commands live in [`testing.md`](testing.md).

## Snapshot (as of 2026-05-10)

- Current focus: **Checkpoint 8.5 — Foundation Hardening**.
- Next checkpoint: **8.5d — Prompt assembly and project memory**.
- Latest completed sub-step: **8.5c — Patch-style editing tool**.
- Planned next order: **8.5d — Prompt assembly and project memory**, then continue through the remaining 8.5 hardening sub-steps before Checkpoint 9.
- Current user-visible behavior: one user prompt can trigger repeated `read_file`, `list_files`, approval-gated or mode-gated `write_file`, approval-gated or mode-gated `edit_file`, approval-gated or mode-gated `shell`, and plan-mode `exit_plan_mode` calls. cawir executes each tool request through a built-in `ToolRegistry`, sends matching `tool_result` blocks back to the active provider, and continues until the provider returns a text answer, the 42-round tool-loop cap is reached, or plan mode asks the REPL for plan approval. Agent progress is emitted as typed `AgentEvent` values from the core loop and rendered by the REPL, while `tool_result` blocks remain session data sent back to providers. In plan mode, either an `exit_plan_mode` call or a plain text final answer is treated as a proposed plan. Tool execution failures, denied mutating actions, denied tool-backed plans, output truncation notices, and shell timeouts are returned inside tool results instead of aborting the turn. `edit_file` changes existing UTF-8 files by exact `old_string` → `new_string` replacement, requires a unique match unless `replace_all` is true, and leaves `write_file` for new files or full rewrites. `read_file` returns at most 64 KiB by default and marks truncated UTF-8-safe prefixes; `list_files` returns at most 200 directory entries before a truncation marker; `shell` caps stdout and stderr separately at 32 KiB and times out after 30 seconds. Startup honors `CAWIR_PROVIDER`, otherwise reuses the saved provider, credential option, and per-provider-plus-auth-option model choices from the OS config directory, then scans providers for usable credentials; if none are configured, startup prompts for a provider before credential setup. Startup prints the selected provider, auth option, credential source, model, session id, and active permission mode. Non-empty conversations are saved as session JSON in the OS data directory after slash-command state changes, exits, and turns; brand-new empty sessions are not written to disk. Sessions store the canonical project path; `cawir --continue` opens the most recently updated non-empty session for the current project, while `cawir --resume <id>` opens a specific session by id. Resumed sessions print the previous conversation, with tool calls and tool results summarized. Slash commands are dispatched through a built-in `CommandRegistry`: bare `/provider` lists provider options; `/provider anthropic|openai|ollama` switches providers inside the REPL after resolving or acquiring a supported credential option from the credentials file, environment, `.env`, API-key prompt, Codex OAuth device-code login, or local no-auth setup, and warns that existing history will be sent to the new provider. Bare `/resume` lists non-empty saved sessions for the current project newest-first, excluding the currently active session; `/resume <id>` switches the running REPL to a saved session and prints its transcript. `/model` shows the current model, the active provider/auth default, and dynamically queried available models where the provider exposes a model-list endpoint; `/model <name>` switches the model for the current provider/auth route and remembers it. `/mode` shows the current permission mode; `/mode default|plan|accept-edits|bypass` switches the current permission mode for the running REPL. OpenAI API-key auth still uses the OpenAI chat-completions endpoint; OpenAI Codex OAuth uses the ChatGPT Codex backend with a Responses-style streaming request that is collected internally before printing. Ollama uses the local `http://localhost:11434/api/chat` endpoint with native Ollama tool calls and defaults to `qwen3:8b`.

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

### 4 — Providers, Auth, Config

- `4a` completed on 2026-05-05. Added concrete OpenAI chat-completions support and extracted the shared `Provider` trait from Anthropic/OpenAI duplication. `ActiveProvider` uses explicit enum delegation for now instead of `Box<dyn Provider>`.
- `4b` completed on 2026-05-05. Added `/provider <anthropic|openai>` as the first argument-taking slash command. Bare `/provider` lists the current and available providers. The REPL now mutates the active provider and API key together, reports bad command usage or missing credentials as user-facing errors, warns that existing history carries across providers, and keeps the session running.
- `4c` completed on 2026-05-05. Added `auth.rs` with concrete `AuthOption` variants for `ApiKey` and `CodexOAuth`; providers now declare accepted credential options and request attachment moved out of provider wire code. Credential resolution checks an OS-appropriate `credentials.json` first, then environment/`.env`; missing credentials can be acquired and saved through a hidden API-key prompt or OpenAI Codex OAuth device-code login. The selected provider and credential option are remembered in `provider.json`; secrets are stored in `credentials.json` with `0600` permissions on Unix. Codex OAuth stores ChatGPT OAuth tokens, refreshes the access token when needed, and routes requests to the ChatGPT Codex backend rather than trying to exchange the device-code `id_token` for an API-key-style model token. The Codex backend requires `stream: true`, so cawir currently collects the SSE response internally and still returns a normal provider response to the agent loop.
- `4d` completed on 2026-05-06. Added `src/ollama.rs` with native Ollama `/api/chat` request/response structs, `qwen3:8b` as the first local tool-capable model, and Ollama tool-call conversion into the internal `MessageContent::ToolUse` shape. Added explicit `AuthOption::None` and a local `ActiveCredential` path so no-auth providers still flow through the same auth boundary instead of special-casing requests. `/provider ollama` and `CAWIR_PROVIDER=ollama` now work with saved provider preference `auth_option: "none"`.
- `4e` completed on 2026-05-06. Added model selection with `/model` and persisted model choices in `provider.json` under a backward-compatible `models` map keyed by provider and auth option. Provider calls now receive the active model from the REPL instead of using hardcoded request constants; providers expose a default model and can dynamically query available models for the active credential route. Ollama lists locally installed models through `/api/tags`, Anthropic lists API models through `/v1/models`, OpenAI API-key auth lists `/v1/models` with a chat-model filter, and OpenAI Codex OAuth lists visible models through the ChatGPT Codex backend `/codex/models?client_version=0.0.0` endpoint. Startup prints the active provider, auth option, credential source, and model.
- Live provider smoke tests are available as ignored tests for Anthropic API key, OpenAI API key, OpenAI Codex OAuth, and local Ollama. See `docs/testing.md` for the opt-in commands.

### 5 — Modes and Plan Mode

- `5a` completed on 2026-05-06. Added `PermissionMode::{Default, Plan, AcceptEdits, Bypass}` plus policy decisions as data in `src/policy.rs`.
- `5b` completed on 2026-05-06. Added `/mode` and `/mode default|plan|accept-edits|bypass` to the concrete REPL slash-command match.
- `5c` completed on 2026-05-06. Threaded the current permission mode into tool execution: `Default` asks on writes and shell, `AcceptEdits` auto-approves writes but asks on shell, `Bypass` allows mutating tools without prompts, and `Plan` denies mutating tools.
- `5d` completed on 2026-05-06. Added plan-mode-only `exit_plan_mode`; the agent returns a proposed plan to the REPL, user approval switches back to `default` mode and continues, and denial is returned as an error tool result. Plain text final answers in plan mode are also treated as proposed plans so models that do not call `exit_plan_mode` still produce an approval prompt. Added a small catastrophic shell-command guard that still applies in `Bypass`.

### 6 — Tool and Slash-Command Registries

- `6a` through `6d` completed on 2026-05-06. Added a built-in `Tool` trait plus `ToolRegistry` so tool name, description, input schema, classification, availability, and execution live with each concrete tool implementation. Added a built-in `Command` trait plus `CommandRegistry` so `/exit`, `/help`, `/provider`, `/model`, and `/mode` dispatch through registered command objects instead of the REPL's hardcoded slash-command match. External registration still waits for MCP, plugins, and skills.

### 7 — Sessions And Resume

- `7a` through `7d` completed on 2026-05-06. Added a serializable `Session` with UUID id, provider, auth option, model, permission mode, canonical project path, timestamps, and message history. Added a REPL-local `Runtime` for non-serializable handles such as the HTTP client, active provider, credential, model preferences, and command registry. Sessions are stored as pretty JSON under the OS data directory using `directories`; `clap` now provides `--resume <id>` and project-scoped `--continue`, bare `/resume` lists saved sessions for the current project, and `/resume <id>` switches sessions inside the REPL.

### 8 — Agent Events

- `8a` through `8c` completed on 2026-05-07. Added `AgentEvent` and `StopReason` as the typed event vocabulary between the core agent loop and surfaces. `agent.rs` now emits user prompt, model request, assistant text, tool request, tool finish, stop, and failure events. `repl.rs` renders those events to preserve the current terminal progress output, while `tool_result` blocks remain separate session data for provider follow-up messages.

### 8.5 — Foundation Hardening

- `8.5a` completed on 2026-05-07. Refactored built-in tool execution into a prepare → central policy/approval → execute flow. File tools now normalize paths against the canonical current project root and deny `read_file`, `list_files`, and `write_file` attempts outside that root, including traversal and absolute outside-project paths. Approval prompting moved out of individual tools and into the REPL-facing tool approval callback; tools still own input parsing, path preparation, and tool-specific hard stops such as catastrophic shell-command denial.
- `8.5b` completed on 2026-05-09. Added output budgets and process limits to built-in tools: `read_file` returns a UTF-8-safe prefix capped at 64 KiB with a visible truncation marker, `list_files` returns at most 200 directory entries with a visible truncation marker, and `shell` caps stdout and stderr independently at 32 KiB while killing commands that exceed a 30-second timeout. Small outputs preserve the previous exact formatting.
- `8.5c` completed on 2026-05-10. Added an approval-gated `edit_file` tool for exact in-place UTF-8 text replacements in existing project files. `edit_file` uses the same project path boundary and mutating-tool policy as `write_file`, rejects missing matches, rejects ambiguous matches unless `replace_all` is true, and keeps `write_file` focused on new files and full rewrites.

## Learnings

- `learnings-rust/` currently includes notes `01` through `37`.
- `learnings-agent/` currently includes notes `01` through `13`.
- New Rust discussions should be distilled into `learnings-rust/*.md`; new agent-design discussions should be distilled into `learnings-agent/*.md`.
