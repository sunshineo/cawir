# Serde-tagged event enums

Checkpoint 8.5g made `AgentEvent` serializable:

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AgentEvent {
    PreToolUse { id: String, name: String, input: Value },
    PostToolUse { id: String, name: String, output_len: usize, is_error: bool, error: Option<String> },
}
```

The serde attribute controls the JSON shape. `tag = "type"` means serde writes a discriminator field that says which enum variant this value is:

```json
{
  "type": "pre_tool_use",
  "id": "toolu_123",
  "name": "read_file",
  "input": { "path": "src/main.rs" }
}
```

`rename_all = "snake_case"` converts Rust variant names like `PreToolUse` into JSON names like `pre_tool_use`.

This gives Rust and JSON different strengths:

- Rust code gets an enum, so `match event` is exhaustive.
- JSON consumers get a stable `type` string, so hooks and surfaces can switch on one field.

That stability is the tradeoff. Once an event is serialized for external consumers, renaming a variant or field can break code outside the crate. Before serialization, renaming an internal enum variant is usually just a Rust refactor.

The same idea applies to smaller enums:

```rust
#[serde(rename_all = "snake_case")]
enum FailureKind {
    ProviderRequest,
    ToolLoopLimit,
}
```

Without `tag = "type"`, this enum serializes as a string such as `"provider_request"`. That is a good fit for enum fields inside a larger event.
