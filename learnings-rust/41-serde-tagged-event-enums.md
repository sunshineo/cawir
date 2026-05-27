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

## Untagged enums for shape-based protocol messages

Checkpoint 14a added a different serde pattern for App Server messages:

```rust
#[derive(Deserialize)]
#[serde(untagged)]
enum ClientMessage {
    Request(ClientRequest),
    Notification(ClientNotification),
}
```

This is useful when the JSON protocol does not include a discriminator field like
`"type"`. JSON-RPC-style messages are identified by their field shape:

```json
{"id":1,"method":"initialize","params":{}}
```

This is a request because it has an `id` and a `method`.

```json
{"method":"initialized","params":{}}
```

This is a notification because it has a `method` and no `id`.

Rust still needs concrete types at compile time. The runtime part is serde trying to
fit incoming JSON into those known Rust shapes:

```rust
let parsed: Result<ClientMessage, serde_json::Error> =
    serde_json::from_value(value);
```

If the JSON matches `ClientRequest`, the result is:

```rust
Ok(ClientMessage::Request(request))
```

If it matches `ClientNotification`, the result is:

```rust
Ok(ClientMessage::Notification(notification))
```

If it matches neither, serde returns `Err(...)`. That is not an exception; it is a
normal `Result` value that protocol code must handle.

After parsing succeeds, the rest of the code is strongly typed again:

```rust
match message {
    ClientMessage::Request(request) => handle_request(request),
    ClientMessage::Notification(notification) => handle_notification(notification),
}
```

The tradeoff:

- `#[serde(tag = "type")]` is explicit and easier to read when we control the JSON
  shape, as with `AgentEvent`.
- `#[serde(untagged)]` fits existing shape-based protocols such as JSON-RPC.
- Untagged parsing is order- and shape-sensitive, so tests are important around
  ambiguous or invalid messages.

## Runtime data, compile-time possibilities

Rust cannot know the shape of a JSON line received over stdin at compile time. What
it can know is the closed set of shapes cawir accepts.

That boundary looks like this:

```text
raw JSON string -> serde runtime parse -> Result<ClientMessage, Error>
```

Invalid runtime data never silently becomes a valid Rust value. The app-server loop
handles both failure classes:

- malformed JSON: return a parse-error response
- valid JSON with the wrong protocol shape: return an invalid-request response

Using `unwrap()` at this boundary would turn bad client input into a panic. Returning
structured protocol errors keeps the server process alive and gives the client a
machine-readable failure.
