# Tool schemas and runtime validation

Tool schemas guide the model, but they are not a hard type system.

The `write_file` schema requires a `content` field:

```json
{
  "required": ["path", "content"]
}
```

The model should follow that schema, but the agent still has to validate tool input at runtime. A model can produce missing fields, wrong field types, or incomplete arguments.

The right shape is:

- Keep the schema strict.
- Validate the received JSON before execution.
- Return validation failures as error `tool_result` blocks so the model can recover.
- Include enough diagnostic detail on failure to debug the actual tool input.

For `write_file`, cawir kept the strict `content` field instead of accepting random aliases like `text` or `file_content`.

That matters because a loose tool contract can hide the real problem. In this case, the better fix for missing `content` was increasing the output budget, not weakening the schema.

## Why runtime validation still matters

Even with a JSON schema, the agent receives tool input as data:

```rust
input: serde_json::Value
```

`Value` means the agent must check the shape before doing anything mutating:

```rust
let content = input
    .get("content")
    .and_then(Value::as_str)
    .ok_or_else(|| Error::ToolInput {
        tool: "write_file".to_string(),
        message: format!("expected input.content to be a string; received input: {}", input),
    })?;
```

This protects the local environment and gives the model a structured chance to recover.
