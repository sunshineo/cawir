# `serde` vs `serde_json`, and typed vs untyped JSON

Checkpoint 3a added Anthropic tool definitions to the request body. That surfaced two related ideas:

1. `serde` and `serde_json` are not the same crate.
2. `serde_json::Value` is flexible, but it gives up compile-time guarantees that typed Rust structs can provide.

## `serde` is the framework, `serde_json` is one format

`serde` defines the traits and derive macros:

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct MessageRequest {
    model: String,
}
```

This means:

> `MessageRequest` knows how to serialize and deserialize.

But `serde` alone does not say **which format** to use. JSON, TOML, YAML, MessagePack, and bincode can all sit on top of serde.

`serde_json` is the JSON-specific crate. It provides:

- `serde_json::Value` — an untyped JSON tree
- `json!({...})` — a macro for building JSON literals in Rust
- helpers like `to_string`, `from_str`, and `from_slice`

Mental model:

- `serde` = the generic interface
- `serde_json` = the JSON toolbox

## What `reqwest` was already doing before 3a

Before 3a, cawir was already sending and receiving JSON:

```rust
.json(&req)
response.json().await?
```

Those methods work because our types derive `Serialize` and `Deserialize` from `serde`.

So the project did **not** add `serde_json` because it suddenly needed JSON request and response parsing. That was already happening through `reqwest` plus the `serde` traits.

What changed in 3a is that our own code needed to hold a JSON-shaped field directly:

```rust
input_schema: Value
```

That is why `serde_json` became a direct dependency.

## Why `input_schema: Value` is convenient

Anthropic's `input_schema` field is naturally a JSON object, so this is easy to write:

```rust
input_schema: json!({
    "type": "object",
    "properties": {
        "path": {
            "type": "string",
            "description": "Path to the file to read."
        }
    },
    "required": ["path"],
    "additionalProperties": false
})
```

This mirrors the API docs almost one-for-one.

That is useful in an early checkpoint because:

- less Rust scaffolding
- fewer helper types
- easier comparison with provider docs
- keeps the step focused on agent behavior, not schema modeling

## The tradeoff: `Value` compiles even when the schema is wrong

`serde_json::Value` means:

> Any valid JSON value can go here.

So these would all compile:

```rust
input_schema: json!(42)
input_schema: json!({"requiredd": ["path"]})
input_schema: json!({"type": "object", "properties": []})
```

They may be invalid tool schemas, but Rust does not know Anthropic's schema rules. It only knows these are valid JSON values.

So with `Value`, Rust catches:

- Rust syntax errors
- non-JSON-serializable data

But Rust does **not** catch:

- misspelled schema keys
- wrong JSON shape for a schema field
- semantic provider mistakes

Those failures show up later, usually as API errors or wrong runtime behavior.

## Typed structs catch more, but only as far as you model them

The alternative is to create Rust types for the schema itself:

```rust
#[derive(Serialize)]
struct ToolDefinition {
    input_schema: ObjectSchema,
}

#[derive(Serialize)]
struct ObjectSchema {
    schema_type: String,
}
```

This catches some mistakes at compile time:

- assigning a number where an `ObjectSchema` is required
- forgetting a required struct field
- using a `bool` where a `String` is expected

But it still does **not** magically validate everything. If `schema_type` is just a `String`, then this still compiles:

```rust
schema_type: "objct".to_string()
```

To make Rust catch more, the types themselves must be more precise:

```rust
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum SchemaType {
    Object,
    String,
}
```

Now `SchemaType::Object` is allowed, but `"objct"` is impossible.

So the spectrum is:

- `Value` — most flexible, weakest guarantees
- loose structs — some structural guarantees
- enums/newtypes/helper constructors — stronger guarantees

## Why 3a still chose `Value`

For one small tool schema, `Value` was the pragmatic choice:

- one tool
- one provider-specific schema
- the docs are already JSON-shaped
- no repetition yet

This matches the project's rule against speculative abstraction. We do not need a mini Rust model of JSON Schema before we have concrete pressure from multiple tools.

## When to refactor away from raw `Value`

Raw `Value` stops being a good fit when:

- several tools repeat the same schema boilerplate
- schema typos become a realistic maintenance problem
- helper constructors would remove repeated string literals cleanly

At that point, a small typed helper layer is reasonable. Not a full schema framework — just enough typing to remove repetition and catch the mistakes we are actually making.

## Explicit dependencies vs transitive dependencies

Even though `reqwest` uses JSON internally, cawir still declares:

```toml
serde_json = "1"
```

That is because cawir imports `serde_json::{Value, json}` directly.

In Rust, if your code uses a crate directly, declare it directly in `Cargo.toml` rather than relying on another crate to bring it in transitively.

## Takeaway

- `serde` provides the generic serialization and deserialization traits.
- `serde_json` provides JSON-specific types and helpers.
- `reqwest` was already turning Rust structs into JSON request bodies and JSON responses back into Rust structs.
- `serde_json::Value` is a useful stepping stone at dynamic JSON boundaries.
- `Value` trades away compile-time schema checking.
- Stronger compile-time guarantees only appear if we model the schema with stronger Rust types.
