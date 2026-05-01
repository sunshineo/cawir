# Output budgets and tool calls

`max_tokens` limits the model's output for a response. Tool-use arguments are part of that output.

That matters for whole-file tools like `write_file`:

```json
{
  "path": "README.md",
  "content": "the full file contents go here"
}
```

If the output budget is too small, the model may not have enough room to emit the complete `content` field. The failure can look like a bad tool call even when the tool schema is reasonable.

cawir originally used:

```rust
max_tokens: 1024
```

That was fine for short chat responses, but too small for creating files. Checkpoint 3h changed this to:

```rust
const MAX_OUTPUT_TOKENS: u32 = 16_384;
```

This is still below Haiku 4.5's documented maximum output, but large enough for modest file creation while avoiding a huge default reservation against output-token-per-minute limits.

## The agent-design lesson

Model output is not only final prose. It can also be structured protocol data:

- tool names
- tool arguments
- patch text
- file contents
- plans or summaries

Any time a tool requires large arguments, the output-token budget becomes part of tool reliability.

Whole-file `write_file` is especially sensitive because the model must emit the entire target file as one argument. A future patch-style edit tool should need fewer output tokens because the model can emit only the changed region.
