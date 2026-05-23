# Agent events and surfaces

Checkpoint 8 introduced a typed event stream between the agent loop and the terminal REPL.

Before this checkpoint, the core loop did two jobs:

```text
orchestrate the model/tool loop
print progress to the terminal
```

Now the core loop emits structured `AgentEvent` values, and the REPL decides how to render them.

## Events are progress, not conversation history

`AgentEvent` is for observers:

```rust
AgentEvent::PreToolUse { id, name, input }
AgentEvent::PostToolUse { id, name, output_len, is_error, error }
AgentEvent::AssistantText { provider, text }
```

These events are useful for terminal rendering, future hooks, logs, JSON output, and a future TUI.

`tool_result` is different. It is model-facing session data:

```rust
MessageContent::ToolResult {
    tool_use_id,
    content,
    is_error,
}
```

The provider needs `tool_result` blocks so the model can continue after a tool call. The session needs them so resumed conversations preserve the model context.

The REPL usually does not need to print the full tool result. It can render a progress summary:

```text
tool result from read_file: 1234 bytes
```

So cawir now keeps two streams separate:

- `AgentEvent`: transient progress and display data.
- `MessageContent::ToolResult`: durable conversation and provider protocol data.

## Why events are an enum

The current event vocabulary is small and owned by cawir:

```rust
pub(crate) enum AgentEvent {
    SessionStart { session_id: String, ... },
    UserPromptSubmit { prompt: String },
    ModelRequestStart { provider: String, model: String },
    ModelRequestFinish { provider: String, model: String, metadata: ProviderMetadata },
    PreToolUse { id: String, name: String, input: Value },
    PostToolUse { ... },
    AssistantText { ... },
    Stop { reason: StopReason },
    StopFailure { kind: FailureKind, message: String, retryable: bool },
    SessionEnd { session_id: String },
}
```

An enum says:

```text
an event is exactly one of these known shapes
```

That fits better than a trait object while the event vocabulary is closed. A trait object is useful when external code must define new event types. cawir is not there yet.

The enum also keeps events as plain data, which is useful for tests and later JSON serialization.

## Producer and consumer boundary

The agent loop accepts an event consumer:

```rust
emit: &mut impl FnMut(AgentEvent)
```

The agent is the producer:

```rust
emit(AgentEvent::ModelRequestStart { ... });
```

The REPL is one consumer:

```rust
fn render_agent_event(event: AgentEvent) {
    match event {
        AgentEvent::PreToolUse { id, name, .. } => {
            println!("tool request: {name} ({id})");
        }
        AgentEvent::AssistantText { provider, text } => {
            println!("{provider}: {text}");
        }
        _ => {}
    }
}
```

Tests can be another consumer by collecting events into a vector.

This is not a thread boundary. It is normal function calling. The callback runs immediately when `agent.rs` emits the event.

## Pre-tool means the raw requested boundary

Checkpoint 8.5g renamed the tool events from `ToolUseRequested` / `ToolUseFinished` to `PreToolUse` / `PostToolUse`.

`PreToolUse` currently fires before `registry.execute(...)`. That means it describes the raw model-requested tool call before cawir has prepared input, applied permission policy, asked for approval, or executed the tool.

This is the earliest useful hook point. A future hook may want to reject a tool call before any local behavior happens. Later, if hooks need the canonical prepared form too, cawir can add a second event point after preparation and policy validation. That pressure does not exist yet.

`PostToolUse` summarizes what happened after execution: original input, output length, error flag, and optional error string. The full tool output still belongs in `MessageContent::ToolResult`, not in the event.

Checkpoint 8.5i added the original input to `PostToolUse` for hooks. A command hook receives one event JSON object on stdin; it should not have to remember a previous `PreToolUse` event just to answer basic questions like:

```text
which file did write_file touch?
was the target path a Rust file?
should a post-write formatter run?
```

This intentionally duplicates a small amount of data between pre-tool and post-tool events. The alternative would force every stateless command hook to build its own correlation store keyed by tool id, which is too much machinery before hooks even exist.

## Why the REPL renders

`agent.rs` owns orchestration:

```text
send provider request
handle provider response
execute tools
append messages
decide when the turn stops
```

`repl.rs` owns the terminal surface:

```text
prompt for input
print progress
ask approval questions
render transcripts
handle slash commands
```

Keeping rendering out of `agent.rs` makes the core loop easier to reuse. A future TUI, JSON command surface, or hook runner can consume the same events without changing model/tool orchestration.

## Serialization is now an event contract

Checkpoint 8 kept `AgentEvent` as internal Rust data. Checkpoint 8.5g moved it closer to an external contract:

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AgentEvent {
    // ...
}
```

This serializes each event with a stable `type` field:

```json
{ "type": "pre_tool_use", "id": "toolu_123", "name": "read_file", "input": { "path": "src/main.rs" } }
```

That shape is easy for hooks, logs, JSON output, a TUI, or a WebSocket surface to consume. The cost is that event names and fields become harder to rename casually. Once external consumers match on `pre_tool_use`, changing that string is an API break.

## App Server before UI polish

OpenAI's Codex App Server write-up sharpened the CP14 direction: the foundation is
not "add a TUI" or "add WebSocket" first. The foundation is a protocol boundary that
lets rich clients drive the same harness without linking to surface internals or
reimplementing the loop:
<https://openai.com/index/unlocking-the-codex-harness/>

For cawir, that means:

- `cawir app-server` starts as stdio JSONL with JSON-RPC-style request, response,
  and notification envelopes.
- WebSocket is a later transport for the same protocol, not the protocol itself.
- `exec`, TUI, and future IDE-style surfaces should reuse the same turn/session path
  behind the app server.
- Approval prompts are bidirectional protocol interactions: the server can ask the
  client for a decision and pause the turn until the client responds.

This also raises the cost of event/protocol churn. Once clients consume an
app-server event, names and payloads become compatibility promises.

## Structured failures serve machines and humans

Checkpoint 8 had `StopFailure { message: String }`. That was readable, but future hooks or alternate surfaces would have had to parse a human string to answer basic questions.

Checkpoint 8.5g changed failures to:

```rust
StopFailure {
    kind: FailureKind,
    message: String,
    retryable: bool,
}
```

`message` stays because people need context. `kind` and `retryable` exist so code can branch without scraping prose. For example, a future surface can render provider failures differently from tool-loop-limit failures, and a future retry policy can look at `retryable`.
