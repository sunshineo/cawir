# Enums for data and control flow

Rust enums are useful for two related but different jobs:

1. Representing the shape of data.
2. Representing the next branch in control flow.

Those should not always be the same enum.

## Data enums

`MessageContent` is a data enum:

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum MessageContent {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}
```

This enum mirrors the JSON blocks inside Anthropic messages. The `#[serde(tag = "type")]` attribute tells serde to use the JSON field named `type` to choose which Rust variant to deserialize.

For example:

```json
{ "type": "text", "text": "hello" }
```

becomes:

```rust
MessageContent::Text {
    text: "hello".to_string(),
}
```

This is similar to a tagged union, discriminated union, or sealed-class hierarchy in other languages. The important Rust detail is that each variant can carry different fields.

## Control-flow enums

`ClaudeResponse` is a control-flow enum:

```rust
enum ClaudeResponse {
    Text(String),
    ToolUse(Vec<MessageContent>),
}
```

This enum does not directly mirror JSON. It represents what cawir should do after a Claude API call:

- `Text(String)` means print and store the assistant reply.
- `ToolUse(Vec<MessageContent>)` means execute a tool and send a `tool_result` back.

The raw HTTP response is first deserialized into a provider-shaped type, then interpreted into `ClaudeResponse`:

```text
Anthropic JSON
    -> MessageResponse
    -> ClaudeResponse
    -> match branch in the agent loop
```

## Why not reuse `MessageContent` for control flow?

`MessageContent` includes `ToolResult`, but Claude should not normally return `tool_result`; cawir sends `tool_result` to Claude. If the agent loop matched directly on `MessageContent`, it would need to handle cases that are irrelevant or impossible for that part of the flow.

`ClaudeResponse::Text(String)` is also intentionally simpler than `MessageContent::Text { text: String }`. If Claude returns multiple text blocks, cawir can join them into one printable `String`.

Keeping separate enums makes the code say what it means:

- `MessageContent`: what data exists inside a message.
- `ClaudeResponse`: what the program should do next.

## Why this is idiomatic Rust

Rust's `match` is exhaustive. If a new variant is added to an enum, the compiler points to every `match` that needs to decide what to do with it.

That makes enums a good fit for explicit state machines and protocol branches. Instead of representing state with loose strings or booleans, the type system carries the possible cases.

## Permission modes as a state machine

Checkpoint 5 added another control-flow enum:

```rust
pub(crate) enum PermissionMode {
    Default,
    Plan,
    AcceptEdits,
    Bypass,
}
```

This is a good enum because cawir is in exactly one permission state at a time, and the set of states is intentionally small and named.

The important part is not just the enum. It is where the enum gets matched:

```rust
match mode {
    PermissionMode::Default => { ... }
    PermissionMode::Plan => { ... }
    PermissionMode::AcceptEdits => { ... }
    PermissionMode::Bypass => { ... }
}
```

For permission logic, exhaustive matching is a safety feature. If a future checkpoint adds:

```rust
PermissionMode::Auto
```

then every policy decision that matches on `PermissionMode` must be updated before the program compiles. That is better than using strings like `"default"` or booleans like `auto_approve_edits`, where adding a new state can silently fall through to the wrong behavior.

Avoid using `_` for important enum policy matches:

```rust
match mode {
    PermissionMode::Default => PermissionDecision::AskUser,
    _ => PermissionDecision::Allow,
}
```

This compiles, but it weakens the safety. If `PermissionMode::Auto` is added later, the compiler will not force a conscious decision for it because `_` already catches it.

Rule of thumb: for closed state machines, list each variant by name. Use `_` only when the exact remaining cases genuinely do not matter.

## Data-bearing control outcomes

Checkpoint 5 also introduced a control-flow enum with data:

```rust
pub(crate) enum TurnOutcome {
    Complete,
    PlanReady(PlanReady),
}
```

`Complete` means the agent turn is done. `PlanReady(PlanReady)` means the agent loop has reached a boundary where the REPL needs to ask the user whether to approve a plan.

This is different from returning only `Result<()>`. `Result` says whether the function succeeded or failed. `TurnOutcome` says what successful thing happened and what the caller should do next.

That shape is common in Rust:

```text
Result<TurnOutcome, Error>
```

Read it as:

```text
The operation can fail with Error.
If it succeeds, it still has one of several meaningful outcomes.
```

Keeping this as an enum makes the caller handle each successful branch explicitly.
