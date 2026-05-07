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
AgentEvent::ToolUseRequested { id, name, input }
AgentEvent::ToolUseFinished { id, name, output_len, is_error, error }
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
    UserPromptSubmit { prompt: String },
    ModelRequestStart { provider: String, model: String },
    ToolUseRequested { id: String, name: String, input: Value },
    ToolUseFinished { ... },
    AssistantText { ... },
    Stop { reason: StopReason },
    StopFailure { message: String },
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
        AgentEvent::ToolUseRequested { id, name, .. } => {
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

## Serialization should wait for hooks

Checkpoint 8 keeps `AgentEvent` as internal Rust data. Checkpoint 9 hooks will likely need event JSON, so `AgentEvent` will probably derive `Serialize` then:

```rust
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AgentEvent {
    // ...
}
```

That should happen when the hook contract is designed, because serialized event names and fields become an external API.

