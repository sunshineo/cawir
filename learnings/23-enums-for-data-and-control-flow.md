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
