# cawir architecture

## Purpose and scope

cawir is a minimal, hand-rolled CLI coding agent in Rust. It is **BYO-model** — the user supplies credentials for whichever provider they want to use (Anthropic, OpenAI, Ollama). The project is a learning vehicle for Rust and agent internals, not a Claude Code competitor.

This document describes the **target architecture** — the shape the codebase is growing toward. Early versions (v0.1, v0.2) implement only subsets; each version extracts one more layer from the stack below, informed by concrete code rather than planning ahead. The point of documenting the target is not to build it all upfront but to know what a speculation would look like when we decide *not* to abstract something yet.

## Influences

The architecture is informed by a study of Claude Code's community deep-dive documentation and source. Claude Code solves a superset of cawir's problem; we borrow its layer decomposition, discard the complexity that exists for Anthropic's production scale (Bash AST parsing, LLM-backed permission classifiers, multi-layer context compaction, etc.), and explicitly skip features that can be added later via well-defined seams.

## The 10-layer stack

```
┌─────────────────────────────────────────────────────────────┐
│ 1. REPL / transport layer                                   │
│    Consumes Stream<AgentEvent>                              │
├─────────────────────────────────────────────────────────────┤
│ 2. Event bus                                                │
│    Typed lifecycle events; hook handlers subscribe;         │
│    PreToolUse handlers can modify input / deny              │
├─────────────────────────────────────────────────────────────┤
│ 3. Agent loop (async + streaming)                           │
│    Emits events at lifecycle points                         │
├──────────────────────────┬──────────────────────────────────┤
│ 4. Tool system           │ 5. Permission layer              │
│    Self-describing tools │    Modes + per-tool validator    │
│    Registry: built-in +  │    (v1 static rules; v2 LLM      │
│    config + MCP + plugin │     classifier slots in here)    │
├──────────────────────────┴──────────────────────────────────┤
│ 6. Hook registry                                            │
│    Loaded from settings; command / prompt / agent handlers  │
├─────────────────────────────────────────────────────────────┤
│ 7. Command registry                                         │
│    Slash commands, built-in + discovered                    │
├─────────────────────────────────────────────────────────────┤
│ 8. Context / session state                                  │
│    Session { id, messages, ... } — pure data,              │
│    serializable from v0.1                                   │
├─────────────────────────────────────────────────────────────┤
│ 9. Prompt assembly                                          │
│    Array: identity + behavior + env + CLAUDE.md layers      │
├─────────────────────────────────────────────────────────────┤
│ 10. Provider / Auth / Credential chain / Settings resolver  │
└─────────────────────────────────────────────────────────────┘
```

## Layer-by-layer

### Layer 1: REPL / transport

The REPL loop reads user input from stdin, hands prompts to the agent loop, consumes the agent's event stream, and renders events to stdout. The REPL is the only layer that knows about terminal formatting; the agent loop below it is transport-agnostic.

**Seam:** swapping the REPL for a different transport (`ratatui` terminal UI, WebSocket, SDK stdio NDJSON, daemon mode) is a one-layer change. The agent emits a stream of typed events; the transport decides how to render or forward them.

### Layer 2: Event bus

A typed pub-sub over agent lifecycle events. Every event has a name, a payload, and handlers return an action that can observe, modify, or block.

Events emitted:

| Cadence | Events |
|---|---|
| Per session | `SessionStart`, `SessionEnd` |
| Per turn | `UserPromptSubmit`, `Stop`, `StopFailure` |
| Per tool call | `PreToolUse`, `PostToolUse` |
| Other | `Notification`, `SubagentStop` (future) |

Handler trait:

```rust
#[async_trait]
pub trait HookHandler: Send + Sync {
    async fn on_event(&self, event: &Event) -> HookAction;
}

pub enum HookAction {
    Continue,                            // no-op
    ModifyInput(serde_json::Value),      // PreToolUse: replace tool input
    Deny(String),                        // PreToolUse: block with reason
    InjectContext(String),               // UserPromptSubmit: prepend to user input
}
```

Handlers come in three flavors: **command** (shell exec, JSON on stdin), **prompt** (LLM-based semantic eval), **agent** (sub-loop invocation). v0.1 ships zero handlers; early versions support command handlers loaded from settings.

**Why first-class (not a callback on `Tool`):** events span more than tool calls — `UserPromptSubmit` fires before any tool, `SessionEnd` fires after. A per-tool callback covers only `PreToolUse`/`PostToolUse`. Centralizing event dispatch also gives us one place to log, trace, and test the lifecycle.

### Layer 3: Agent loop

The core async loop. Streams model output, dispatches tool calls, emits lifecycle events.

```rust
pub fn run(
    session: &mut Session,
    runtime: &Runtime,
) -> impl Stream<Item = AgentEvent>
```

`Runtime` holds the non-serializable handles: `reqwest::Client`, the `EventBus`, the `ToolRegistry`, the `SettingsResolver`. `Session` holds only serializable conversation state. This split is the critical v0.1 discipline (see layer 8).

Properties:

- **Streams model output** as it arrives (decided for v1.0 — stubbed as buffered until then).
- **Tool calls pause the stream** and resume within the same model response cycle — not a new top-level model call per tool.
- **Cancelable** — dropping the stream aborts in-flight tool work cleanly.

### Layer 4: Tool system

Tools are self-describing. Each declares its name, JSON schema, a description string for prompt assembly, and its execution logic. A registry holds them; dispatch looks up by name.

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    async fn execute(&self, input: serde_json::Value) -> ToolResult;

    // Optional per-tool semantic check (layer 5's inner half)
    fn validate(&self, _input: &serde_json::Value) -> Validation {
        Validation::Allow
    }

    // Optional render hook for the transport layer
    fn render(&self, result: &ToolResult) -> String {
        result.content.clone()
    }
}

pub struct ToolRegistry { /* name -> Arc<dyn Tool> */ }
```

**Registry is multi-sourced.** Populated at startup from: built-in tools, config-declared tools, MCP-discovered tools (later), plugin-discovered tools (later). The registry doesn't care about source.

### Layer 5: Permission layer

Two concerns: the session's **permission mode** (coarse-grained policy), and each tool's **semantic validation** (fine-grained input check).

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

The full permission check for a tool call:

```
mode.check(tool) ∧ tool.validate(input) ∧ hooks.PreToolUse(input)
```

All three must pass. Any layer can deny; PreToolUse hooks can also modify input before execution.

**Plan mode specifically:**

1. While `PermissionMode::Plan`, all mutating tools (write, edit, shell) return `Deny("plan mode")`.
2. A special built-in `ExitPlanMode` tool is registered only in plan mode. Its "execution" doesn't mutate — it emits `AgentEvent::PlanReady { plan }` upward.
3. REPL handles `PlanReady` by rendering the plan and prompting the user for approval.
4. On approve, mode switches to previous (or user-selected), loop resumes.

**Seam:** `PermissionMode::Auto` (v2) with an LLM classifier fits the same `mode.check(tool)` interface. The classifier implementation lives behind `Auto` without changing other layers.

### Layer 6: Hook registry

Maps events to handlers. Loaded from `settings.json` at startup, merged via the settings resolver. Nothing more than a typed dispatch table in v1; complexity is in the handler implementations, not the registry itself.

### Layer 7: Command registry

Slash commands handled locally by the REPL instead of sent to the model. Built-ins: `/exit`, `/clear`, `/help`, `/provider`, `/mode`, `/resume`. Additional commands discovered from `.claude/commands/*.md` or plugins later.

**Seam:** identical structure to the tool registry — discovery-from-sources pattern generalizes.

### Layer 8: Context / session state

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

### Why Session is serializable from v0.1

Adding `#[derive(Serialize, Deserialize)]` later forces a painful refactor because the moment a type holds a `reqwest::Client` or channel handle, it can't derive serde. The discipline is cheap when the code is ~100 lines; it's real work at ~3000 lines.

What this commitment buys:

- **Clean interior/exterior boundary.** Agent loop takes `&mut Session` (data) and `&Runtime` (handles). Forces separation from day one.
- **Session ID is first-class.** Available to hooks, logs, events immediately — not retrofitted at save-time. Avoids the known Claude Code bug where a running session can't see its own ID.
- **Future `/resume` is ~20 lines:** `read JSON → deserialize → pass to loop`.
- **Debugging wins immediately:** `dbg!(&session)` and `serde_json::to_string_pretty(&session)` work from v0.1.

What this commitment does **not** mean:

- v0.1 does not write sessions to disk.
- v0.1 does not implement `/resume`.
- Resume does not replay tool side effects — it only continues the conversation.

**Seam:** compaction strategies (micro-compact, full-compact, memory extraction) slot in as `impl CompactionStrategy for X { fn compact(&self, s: &mut Session); }`.

### Layer 9: Prompt assembly

The system prompt is assembled as an array of named sections, not a monolithic string.

```rust
pub struct SystemPrompt {
    pub sections: Vec<PromptSection>,
}

pub struct PromptSection {
    pub name: String,
    pub content: String,
    pub cache_breakpoint: bool,
}
```

Typical sections: `identity`, `behavior`, `env` (cwd, git branch, OS), `memory` (CLAUDE.md content loaded from user/project hierarchy).

Tool definitions are sent via the provider's dedicated `tools:` API channel, **not** inlined in the prompt text. This keeps the prompt prefix cache-stable when the tool set changes.

**Seam:** prompt caching. `cache_breakpoint: bool` is a hint that the provider layer translates into provider-specific cache directives (`cache_control` for Anthropic) when we wire it in later.

### Layer 10: Provider, Auth, Credential chain, Settings resolver

**Provider** — wire format only, orthogonal to credentials:

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

**AuthMethod** — credential attachment only, orthogonal to provider:

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

**Credential chain** resolves the credential for the selected provider through: macOS Keychain → environment variable → `.env` file.

**Settings resolver** resolves any config key by walking: `./.claude/settings.local.json` → `./.claude/settings.json` → `~/.claude/settings.json`, deep-merging in precedence order. Used by the hook registry, tool registry, command registry, MCP config (later), and any other component that reads configuration.

## Commit-level decisions

These are decisions we commit to from v0.1 — not "grown into."

| # | Decision | Rationale |
|---|---|---|
| 1 | Agent loop returns `Stream<AgentEvent>`; REPL consumes it | Sync loop blocks streaming + cancellation |
| 2 | Each tool is a `Tool` trait impl with name, schema, description, execute | Free functions don't carry schema; no cohesion |
| 3 | Permission = `PermissionMode` enum + `Tool::validate` method + PreToolUse hooks | Global allow/deny has wrong granularity |
| 4 | Context = `Session` struct (pure data). No compaction in v1. | Simpler; compaction is last-resort |
| 5 | `Provider` and `AuthMethod` are separate traits; providers declare which auths they accept | Single trait couples wire format with auth |
| 6 | `main.rs` thin; real code in `lib.rs` so tests can reach it | All-in-main is untestable |
| 7 | Error type = `thiserror` enum from v0.1 | `anyhow` is too opaque for a learning project |
| 8 | Agent loop uses an `EventBus`. Lifecycle events are defined as an enum from v0.1, even if only a stub bus is wired. | Retrofitting event emission points is painful |
| 9 | `Session` struct derives `Serialize`/`Deserialize` from v0.1, whether we persist it or not | `/resume` later is an implementation, not a schema migration |
| 10 | One `SettingsResolver` handles every config lookup (user → project → local) | Avoids one-off "where do I read this from" decisions everywhere |

## Extension seams

Things not built in v1 but with a clear place in the architecture:

| Future capability | Where it plugs in |
|---|---|
| **MCP tools** | `ToolRegistry` accepts dynamically-discovered tools; MCP is one source (a `McpTool` impl wrapping a server connection) |
| **Plugins** | `ToolRegistry`, `HookRegistry`, `CommandRegistry` all accept discovered entries; plugin loader walks a directory |
| **Subagents** | Composable — a `SubAgent` tool instantiates another `agent::run(...)` loop on a nested `Session` |
| **Auto-mode classifier** | Add `PermissionMode::Auto`; its implementation calls an LLM via `Provider` |
| **Context compaction** | Strategy pattern: `fn compact(&self, s: &mut Session)` |
| **Session memory extraction** | `MemoryStore` consumes `Session` at `SessionEnd`, writes consolidated memory |
| **Richer terminal UI** | Replace REPL with a `ratatui` consumer of the same `Stream<AgentEvent>` |
| **Remote transports (WebSocket, SSE)** | Another consumer of the same stream |
| **Daemon / headless mode** | Another consumer of the same stream; supervisor wrapping `agent::run` |
| **Prompt caching** | Each `PromptSection` carries `cache_breakpoint: bool`; provider translates |
| **Streaming** | Already the design; providers emit token-by-token into `CompletionStream` |

## Target module layout

```
src/
├── main.rs              thin: arg parse → launch repl::run()
├── lib.rs               re-exports the public library surface
├── agent.rs             AgentLoop, emits Stream<AgentEvent>
├── event.rs             EventBus, AgentEvent, HookAction types
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
├── credential.rs        Keychain → env → .env lookup chain
├── tool/
│   ├── mod.rs           Tool trait + ToolRegistry
│   ├── read_file.rs
│   ├── write_file.rs    (later)
│   └── shell.rs         (later)
├── permission.rs        PermissionMode + Validation types
├── hook.rs              HookRegistry, HookHandler trait, handler impls
├── command.rs           CommandRegistry + built-in slash commands
├── session.rs           Session, Message, ContentBlock, SessionId — serde types
├── prompt.rs            SystemPrompt, PromptSection, assembly logic
├── settings.rs          SettingsResolver (user / project / local merge)
├── config.rs            Config struct (persisted across sessions)
└── error.rs             thiserror enum
```

Early versions will have only a subset of these files. Each roadmap milestone extracts one more layer into its own module, informed by concrete code.

## Growth approach

v0.1 ships with a few files — a hard-coded Anthropic POST, a minimal REPL, a `Session` struct. Each subsequent version extracts one more layer from the target stack, informed by concrete code. When and in what order we extract each layer is a roadmap question, covered separately.

The discipline: **no speculative abstractions, but the target shape is known.** We don't build `Provider` trait in v0.1 because we only have one provider; we do know where it will live when we extract it from two concrete impls at v0.6. That's the difference between "grown organically" and "improvised."
