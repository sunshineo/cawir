# Tool loops, context growth, and caching

Coding agents do not make one model call per user prompt. Once tools exist, one user prompt can become a loop:

```text
user prompt
-> model asks for tool(s)
-> agent executes tools
-> agent sends tool_result blocks back
-> model asks for more tools or returns final text
```

This loop is the core difference between "chat with a model" and "agent that can inspect or change the environment."

## Tool results become context

A tool result is not just printed locally. It is appended to the conversation history and sent back to the model so the model can reason over it.

For `read_file`, this means the full file contents become part of future API requests:

```text
read_file("docs/roadmap.md") -> tool_result contains roadmap text
next model call includes that roadmap text in history
```

This is necessary for correctness: the model cannot answer from a file unless the content is sent back. But it also means every file read increases the prompt size for later loop iterations.

## Terminal output is not token usage

Suppressing local printing makes the terminal readable, but it does not reduce API usage.

cawir changed tool display from dumping full file contents to:

```text
tool result from read_file: 1234 bytes
```

That only affects stdout. The full result still goes into `history` and is still sent to the model.

## Repeated tool rounds can hit rate limits

Without caching, each loop iteration resends the full history:

```text
request 1: user prompt
request 2: user prompt + tool_result(file A)
request 3: user prompt + tool_result(file A) + tool_result(file B)
request 4: user prompt + file A + file B + file C
```

This can quickly exceed input-tokens-per-minute limits, especially when file reads are large and several calls happen inside one minute.

## Prompt caching is a protocol-level mitigation

Anthropic prompt caching lets repeated request prefixes be reused between calls. For cawir's current Anthropic request, automatic caching is enabled with a top-level field:

```json
{
  "cache_control": { "type": "ephemeral" }
}
```

This does not change the logical conversation. It changes how Anthropic processes repeated prefixes across calls.

Caching helps most when:

- The same tools, system prompt, and earlier messages are sent repeatedly.
- Tool loops make several calls close together.
- Large tool results remain stable across subsequent requests.

Caching does not remove the need for context budgeting. Large file reads still enter history, and provider-specific rate-limit rules may still count some cached tokens.

## Loop caps are a safety rail

Agents need a hard stop so a model cannot ask for tools forever.

cawir currently uses:

```rust
const MAX_TOOL_ROUNDS: usize = 42;
```

Each model response containing one or more tool calls counts as one round. If the cap is exceeded, cawir aborts the turn and rolls back that user prompt's history.

The cap protects cost and rate limits, but it is not a context-management strategy. It prevents runaway loops; it does not make each loop iteration smaller.

## Future context-budgeting work

Prompt caching and loop caps are not enough for a mature coding agent. The next pressure points are:

- File-size caps so one `read_file` cannot inject a huge file by default.
- Ranged reads so the model can inspect relevant slices instead of entire files.
- Better directory filtering so generated files, build outputs, and large docs are avoided unless explicitly needed.
- Usage observability so `input_tokens`, `cache_creation_input_tokens`, and `cache_read_input_tokens` are visible during learning.
- 429 handling that can read `retry-after` and decide whether to wait, retry, or return a clearer recoverable error.

The general lesson: agent correctness requires giving the model enough context, but agent engineering is mostly about controlling how much context enters the loop and how often it is resent.
