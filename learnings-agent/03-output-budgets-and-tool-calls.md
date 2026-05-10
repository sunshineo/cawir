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

## Tool results also need budgets

Checkpoint 8.5b added budgets in the other direction: tool results flowing back into the model.

This matters because a single tool call can inject a large amount of text into conversation history:

- a huge source file from `read_file`
- a directory with thousands of entries from `list_files`
- a failing test command with many pages of logs from `shell`

The key rule is not just "truncate." It is:

> truncation must be visible to the model

If cawir silently cuts output, the model may treat incomplete context as complete. A visible marker lets the model recover by asking for a narrower read, a more specific directory, or a filtered shell command.

Current defaults:

- `read_file`: first 64 KiB, preserving a valid UTF-8 boundary
- `list_files`: first 200 directory entries
- `shell`: first 32 KiB of stdout and first 32 KiB of stderr

These are not universal truths; they are conservative starting defaults. The design point is that every broad tool needs an output budget and an honest way to say "you are seeing only part of this."

## Tool contracts should be explicit about text

`read_file` is currently a UTF-8 text tool. That is not the same as "English text" or "ASCII only": UTF-8 fully supports comments and source text in Chinese and other languages.

The limitation is about encoding. A strict UTF-8 `read_file` should reject binary files and legacy-encoded files rather than silently replacing bytes, because the model expects exact source text.

If cawir later needs to inspect non-UTF-8 files, that should probably be a different contract, such as:

- a binary or hex preview tool
- a metadata/encoding inspection tool
- an explicitly lossy preview mode

Keeping `read_file` strict makes the common source-code path predictable while leaving room for a separate tool with different semantics.
