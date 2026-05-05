# cawir architecture

## Purpose and scope

cawir is a minimal, hand-rolled CLI coding agent in Rust. It is **BYO-model** — the user supplies credentials for whichever provider they want to use (Anthropic, OpenAI, Ollama). The project is a learning vehicle for Rust and agent internals, not a Claude Code competitor.

This document describes the **target architecture** — the shape the codebase is growing toward. Early checkpoints implement only subsets; each checkpoint extracts one more component from what follows, informed by concrete code rather than planning ahead. The point of documenting the target is not to build it all upfront, but to know what a speculation would look like when we decide *not* to abstract something yet.

Current implementation state lives in [`status.md`](status.md). Checkpoint sequence lives in [`roadmap.md`](roadmap.md).

## Influences

The architecture is informed by a study of Claude Code's community deep-dive documentation and source. Claude Code solves a superset of cawir's problem; we borrow its component decomposition, discard the complexity that exists for Anthropic's production scale (Bash AST parsing, LLM-backed permission classifiers, multi-layer context compaction, etc.), and explicitly skip features that can be added later via well-defined seams.

## Component architecture

cawir is a **component graph**, not a linear stack. The agent loop is the orchestrator at the center; other components fan out from it. Five functional groups:

```
┌────────────────────────────────────────────────────────────┐
│  Surface       REPL · slash-command parser                 │
├────────────────────────────────────────────────────────────┤
│  Core engine   Agent loop · Session · Prompt assembly      │
│                (Events are a datatype emitted here; hook   │
│                dispatch is one call the agent loop makes)  │
├────────────────────────────────────────────────────────────┤
│  Capabilities  Tool registry + Tool trait                  │
│                Hook registry + Handler impls               │
├────────────────────────────────────────────────────────────┤
│  Policy        Permission modes + per-tool validators      │
├────────────────────────────────────────────────────────────┤
│  External      Provider · AuthMethod · Credential chain    │
│                SettingsResolver                            │
└────────────────────────────────────────────────────────────┘
```

### Component flow at runtime

```
              ┌──────┐
              │ REPL │──── parses /commands locally
              └──┬───┘
     user input  │   ▲ Stream<AgentEvent>
                 ▼   │
             ┌─────────────┐
  Prompt ◄───┤ Agent loop  ├──► HookRegistry ──► handlers
  assembly   │             │    (sync dispatch)  (cmd/prompt/agent)
             └──┬────┬──┬──┘
    mutates/    │    │  │ dispatches tool call
    reads       │    │  ▼
                │    │  ToolRegistry ──► Tool.execute()
                │    │                      ▲
                │    │          Permission check
                │    │          (mode + Tool.validate +
                │    │           PreToolUse hooks)
                │    │
                │    │ calls model
                │    ▼
                │    Provider + AuthMethod ──► HTTP
                │              │
                │              └──► CredentialChain
                │                   (credentials.json/env/.env)
                │
                ▼ (pure data; serde)
              Session { id, messages, ... }

Read-everywhere:  SettingsResolver
                    ◄── ./.claude/settings.local.json
                    ◄── ./.claude/settings.json
                    ◄── ~/.claude/settings.json
```

Key properties of this shape:

- The agent loop is the **only** component that emits lifecycle events. Hooks consume them synchronously (can modify/deny); the REPL consumes them asynchronously (observation only).
- The permission check is a conjunction: `mode.check(tool) ∧ tool.validate(input) ∧ hooks.PreToolUse(input)`. Any of the three can deny; PreToolUse can also modify input.
- `Session` is pure data — it derives `Serialize`/`Deserialize`. The non-serializable runtime handles (`reqwest::Client`, registries, settings) live in a separate `Runtime` struct passed alongside.
- `SettingsResolver` is a read-everywhere utility, not part of any single group.

## The five groups

### 1. Surface

Where user input arrives and agent output is rendered. Tightly coupled components — the REPL parses slash commands inline; they are not a separate subsystem.

**REPL** — reads user input from stdin, submits to the agent loop, consumes `Stream<AgentEvent>`, renders events to stdout. The only component that knows about terminal formatting.

**Slash-command parsing** — intercepts `/commands` before they reach the agent loop. Built-ins: `/exit`, `/clear`, `/help`, `/provider`, `/mode`, `/resume`. Additional commands can be loaded from `.claude/commands/*.md` or plugins later.

**Seam — alternate transports.** Swapping the REPL for a different consumer of `Stream<AgentEvent>` (`ratatui` TUI, WebSocket server, daemon mode, SDK stdio NDJSON) is a Surface-only change. The agent loop is transport-agnostic.

### 2. Core engine

The orchestrator and what it manipulates. Three components work together: the agent loop (control flow), `Session` (data it mutates), and prompt assembly (input it constructs for the model). Events are defined here; hook dispatch is a call the agent loop makes.

**Agent loop:**

```rust
pub fn run(
    session: &mut Session,
    runtime: &Runtime,
) -> impl Stream<Item = AgentEvent>
```

`Runtime` holds non-serializable handles: `reqwest::Client`, `ToolRegistry`, `HookRegistry`, `SettingsResolver`. `Session` holds only serializable conversation state.

Lifecycle:

- Emits events at well-defined points: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`, `StopFailure`, `SessionEnd`.
- Dispatches tool calls to the `ToolRegistry`.
- Calls the `Provider` + `AuthMethod` to talk to the model, streaming response tokens.
- Cancelable — dropping the stream aborts in-flight work cleanly.

**Tool-loop cap.** Agent turns must have a bounded number of tool rounds so a bad prompt, model loop, or protocol bug cannot burn tokens indefinitely. The current read-only checkpoint uses `MAX_TOOL_ROUNDS = 42`: each model response containing one or more tool calls counts as one round. Exceeding the cap returns a typed error and rolls back the current user turn. This is a runtime safety rail, not a provider abstraction; later settings can make the cap configurable without changing the loop shape.

**Events and hook dispatch** — events are a typed enum defined in the core engine. Two consumption patterns of the same events:

```rust
pub enum AgentEvent {
    SessionStart { id: SessionId },
    UserPromptSubmit { prompt: String },
    PreToolUse { tool: String, input: serde_json::Value },
    PostToolUse { tool: String, result: ToolResult },
    Stop,
    StopFailure { error: String },
    SessionEnd,
}

pub enum HookAction {
    Continue,                            // no-op
    ModifyInput(serde_json::Value),      // PreToolUse only
    Deny(String),                        // PreToolUse only
    InjectContext(String),               // UserPromptSubmit only
}
```

When the loop reaches an event point it **synchronously** calls `hook_registry.dispatch(&event).await`, honors the returned `HookAction` (blocks, modifies input, or continues), and then **asynchronously** yields the same event into the output `Stream` for the REPL to render. Two interaction patterns, one event vocabulary.

**Prompt assembly** — the system prompt is an array of named sections, not a monolithic string:

```rust
pub struct SystemPrompt {
    pub sections: Vec<PromptSection>,
}

pub struct PromptSection {
    pub name: String,              // "identity", "behavior", "env", "memory"
    pub content: String,
    pub cache_breakpoint: bool,
}
```

Typical sections: `identity`, `behavior`, `env` (cwd, git branch, OS), `memory` (CLAUDE.md content loaded from user/project hierarchy).

Tool definitions go via the provider's dedicated `tools:` API channel, **not** inlined into prompt text. This keeps the prompt prefix cache-stable when the tool set changes.

**Seam — prompt caching.** `cache_breakpoint: bool` is a hint the `Provider` layer later translates into provider-specific directives (`cache_control` for Anthropic). Wiring is additive; no restructuring needed.

**Session** — the data the agent loop mutates. See [Session as pure data](#session-as-pure-data) below for the full type definition and rationale.

**Current module ownership during early checkpoints.** Until the target modules are extracted, `session.rs` owns durable serde conversation data: `Message` and `MessageContent`. These are the types that can live in `history` and later be serialized for `/resume`. `lib.rs` still owns the current REPL orchestration, Anthropic HTTP call, and temporary request/response structs such as `MessageRequest`, `MessageResponse`, and `ClaudeResponse`. Those are runtime/wire/control-flow details, not durable session data. When provider extraction happens, Anthropic-specific request/response structs move out of `lib.rs`; session data stays provider-neutral unless real multi-provider pressure forces a schema change.

### 3. Capabilities

Two registries for things the agent can invoke. Both follow the same pattern: a trait + a registry + multiple population sources.

**Tool registry + Tool trait.** Tools are self-describing — name, JSON schema, description (for prompt assembly), and execution logic:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value) -> ToolResult;

    fn validate(&self, _input: &serde_json::Value) -> Validation {
        Validation::Allow
    }

    fn render(&self, result: &ToolResult) -> String {
        result.content.clone()
    }
}

pub struct ToolRegistry { /* name -> Arc<dyn Tool> */ }
```

Populated at startup from: built-in tools, config-declared tools, MCP-discovered tools (later), plugin-discovered tools (later). The registry doesn't care about source.

**Hook registry + Handler impls.** Handlers are subscribers to lifecycle events. One trait, three flavors of implementation:

```rust
#[async_trait]
pub trait HookHandler: Send + Sync {
    async fn on_event(&self, event: &AgentEvent) -> HookAction;
}
```

Handler flavors:

- **Command** — runs a shell command; event JSON goes on stdin; the handler reads `HookAction` from exit code + stdout.
- **Prompt** — LLM-based semantic evaluation.
- **Agent** — full sub-loop invocation.

The `HookRegistry` is a dispatch table: `event_kind → Vec<Arc<dyn HookHandler>>`. Populated at startup from `settings.json` (via the settings resolver) and plugins (later). Early versions can ship with no handlers at all.

### 4. Policy

One component: **permission**. A coarse-grained mode plus a fine-grained per-tool validator.

```rust
pub enum PermissionMode {
    Default,       // ask on mutating tools
    Plan,          // deny mutating; require ExitPlanMode tool
    AcceptEdits,   // auto-approve writes; still ask on shell
    Bypass,        // allow everything (explicit opt-in, dangerous)
}

pub enum Validation {
    Allow,
    Deny(String),
    AskUser(String),
}
```

**Plan mode specifically:**

1. While `PermissionMode::Plan`, mutating tools (write, edit, shell) return `Deny("plan mode")`.
2. A built-in `ExitPlanMode` tool is registered only in plan mode. Its "execution" doesn't mutate — it emits `AgentEvent::PlanReady { plan }` upward.
3. REPL handles `PlanReady` by rendering the plan and prompting for approval.
4. On approve, mode switches and the loop resumes.

**Seam — auto-mode classifier.** `PermissionMode::Auto` backed by an LLM classifier fits the same `mode.check(tool)` interface. The classifier calls out via the `Provider` component without changing anything else.

### 5. External

Everything that reaches outside the process: model APIs, credentials, config files. Four components with orthogonal responsibilities.

**Provider** — wire format per model backend. Orthogonal to credentials.

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn accepts_auth(&self, kind: &AuthMethodKind) -> bool;

    async fn complete(
        &self,
        req: CompletionRequest,
        auth: &dyn AuthMethod,
    ) -> Result<CompletionStream>;
}
```

**AuthMethod** — how credentials attach to an HTTP request. Orthogonal to provider.

```rust
pub trait AuthMethod: Send + Sync {
    fn kind(&self) -> AuthMethodKind;
    fn apply(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder;
}

pub enum AuthMethodKind {
    ApiKey,
    OAuthToken,
    None,
}
```

Provider × Auth compatibility matrix (per ToS):

| Provider | Accepts | Notes |
|---|---|---|
| Anthropic | `ApiKey` | Subscription OAuth banned by ToS |
| OpenAI | `ApiKey`, `OAuthToken` | Codex subscription OAuth officially supported |
| Ollama | `None` | Local, no auth |

**Credential chain** resolves credentials for a given provider: credentials file → environment variable → `.env` file. The chain is queried by the auth layer at request time.

**SettingsResolver** resolves any config key by walking: `./.claude/settings.local.json` → `./.claude/settings.json` → `~/.claude/settings.json`, deep-merging in precedence order. Read-everywhere utility — used by the hook registry, tool registry, slash-command loading, and any other component that reads configuration.

## Session as pure data

The type definition:

```rust
#[derive(Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub created_at: SystemTime,
    pub working_dir: PathBuf,
    pub provider: ProviderName,
    pub model: String,
    pub messages: Vec<Message>,
}

#[derive(Serialize, Deserialize)]
pub enum Message {
    User(String),
    Assistant { content: Vec<ContentBlock> },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
}

#[derive(Serialize, Deserialize)]
pub enum ContentBlock {
    Text(String),
    ToolUse { id: String, name: String, input: serde_json::Value },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct SessionId(uuid::Uuid);
```

### Why serializable from the start

Adding `#[derive(Serialize, Deserialize)]` later forces a painful refactor — the moment a type holds a `reqwest::Client` or channel handle, it can't derive serde. The discipline is cheap when the code is ~100 lines; it's real work at ~3000 lines.

What this commitment buys:

- **Clean interior/exterior boundary.** Agent loop takes `&mut Session` (data) and `&Runtime` (handles). Forces the split from day one.
- **Session ID is first-class.** Available to hooks, logs, and events immediately — not retrofitted at save-time. Avoids the known Claude Code bug where a running session can't see its own ID.
- **Future `/resume` is ~20 lines:** `read JSON → deserialize → pass to loop`.
- **Debugging wins immediately:** `dbg!(&session)` and `serde_json::to_string_pretty(&session)` work from the start.

What this commitment does **not** mean:

- cawir does not need to write sessions to disk immediately.
- cawir does not need to implement `/resume` immediately.
- Resume does not replay tool side effects — it only continues the conversation.

**Seam — compaction strategies** (micro-compact, full-compact, memory extraction) slot in as `impl CompactionStrategy for X { fn compact(&self, s: &mut Session); }`.

## Commit-level decisions

These are start-of-project decisions — not things we wait to "grow into."

| # | Decision | Rationale |
|---|---|---|
| 1 | Agent loop returns `Stream<AgentEvent>`; REPL consumes it | Sync loop blocks streaming + cancellation |
| 2 | Each tool is a `Tool` trait impl with name, schema, description, execute | Free functions don't carry schema; no cohesion |
| 3 | Permission = `PermissionMode` enum + `Tool::validate` method + PreToolUse hooks | Global allow/deny has wrong granularity |
| 4 | Context = `Session` struct (pure data). No compaction in the early versions. | Simpler; compaction is last-resort |
| 5 | `Provider` and `AuthMethod` are separate traits; providers declare which auths they accept | Single trait couples wire format with auth |
| 6 | `main.rs` thin; real code in `lib.rs` so tests can reach it | All-in-main is untestable |
| 7 | Error type = `thiserror` enum from the start | `anyhow` is too opaque for a learning project |
| 8 | Agent loop emits typed `AgentEvent` values at lifecycle points; hook dispatch runs synchronously before each event flows to the REPL stream. Event enum defined from the start. | Retrofitting event emission points later is painful |
| 9 | `Session` struct derives `Serialize`/`Deserialize` from the start, whether we persist it or not | `/resume` later is an implementation, not a schema migration |
| 10 | One `SettingsResolver` handles every config lookup (user → project → local) | Avoids one-off "where do I read this from" decisions everywhere |

## Extension seams

Things not built yet but with a clear place in the architecture:

| Future capability | Where it plugs in |
|---|---|
| **MCP tools** | `ToolRegistry` accepts dynamically-discovered tools; MCP is one source (a `McpTool` impl wrapping a server connection) |
| **Plugins** | `ToolRegistry` and `HookRegistry` accept discovered entries; plugin loader walks a directory. Slash commands similarly loadable from files. |
| **Subagents** | Composable — a `SubAgent` tool instantiates another `agent::run(...)` loop on a nested `Session` |
| **Auto-mode classifier** | Add `PermissionMode::Auto`; its implementation calls an LLM via `Provider` |
| **Context compaction** | Strategy pattern: `fn compact(&self, s: &mut Session)` |
| **Session memory extraction** | `MemoryStore` consumes `Session` at `SessionEnd`, writes consolidated memory |
| **Richer terminal UI** | Replace REPL with a [Ratatui](https://ratatui.rs) consumer (Crossterm as the cross-platform backend) of the same `Stream<AgentEvent>`. Ratatui is the flagship Rust TUI library (immediate-mode rendering; used in production by OpenAI's Codex CLI, Netflix, AWS). Only the Surface layer changes — agent loop, event bus, tools all untouched. |
| **Remote transports (WebSocket, SSE)** | Another consumer of the same stream |
| **Daemon / headless mode** | Another consumer of the same stream; supervisor wrapping `agent::run` |
| **Prompt caching** | Each `PromptSection` carries `cache_breakpoint: bool`; provider translates |
| **Streaming** | Already the design; providers emit token-by-token into `CompletionStream` |

## Target module layout

```
src/
├── main.rs              thin: arg parse → launch repl::run()
├── lib.rs               re-exports the public library surface
├── agent.rs             agent loop, emits Stream<AgentEvent>
├── event.rs             AgentEvent, HookAction enums
├── repl.rs              stdin reader, slash-command parser, event consumer
├── provider/
│   ├── mod.rs           Provider trait + request/response types
│   ├── anthropic.rs
│   ├── openai.rs        (later)
│   └── ollama.rs        (later)
├── auth/
│   ├── mod.rs           AuthMethod trait + AuthMethodKind
│   ├── api_key.rs
│   └── oauth.rs         (later)
├── credential.rs        credentials file → env → .env lookup chain
├── tool/
│   ├── mod.rs           Tool trait + ToolRegistry
│   ├── read_file.rs
│   ├── write_file.rs    (later)
│   └── shell.rs         (later)
├── permission.rs        PermissionMode + Validation types
├── hook.rs              HookRegistry, HookHandler trait, handler impls
├── session.rs           Session, Message, MessageContent, SessionId — serde types
├── prompt.rs            SystemPrompt, PromptSection, assembly logic
├── settings.rs          SettingsResolver (user / project / local merge)
├── config.rs            Config struct (persisted across sessions)
└── error.rs             thiserror enum
```

Early versions have only a subset of these files. Each roadmap milestone extracts one more component into its own module, informed by concrete code.

## Growth approach

The first versions ship with only a few files — a hard-coded Anthropic POST, a minimal REPL, a `Session` struct. Each subsequent version extracts one more component from the target architecture, informed by concrete code. When and in what order we extract each component is a roadmap question, covered separately.

The discipline: **no speculative abstractions, but the target shape is known.** We don't build the `Provider` trait while there is still only one provider; we do know where it will live when a second provider creates real extraction pressure. That's the difference between "grown organically" and "improvised."
