# Streaming provider events

Checkpoint 10 changed provider calls from "wait for the full answer, then render" to "emit deltas while still accumulating the final response."

## Stream deltas are progress, not durable session data

The provider emits `ProviderEvent` values for streaming progress:

```text
TextDelta
ToolUseStart
ToolUseInputDelta
```

The provider still returns a final `ProviderResponse` containing full text or complete tool-use blocks. That split is important:

```text
stream deltas -> UI and hooks can observe progress
final response -> session history and next provider request
```

Durable sessions should not store every token delta. They store the final assistant text and complete tool calls because those are the conversation state the next model request needs.

## Tool calls still execute after completion

Tool-use streaming exposes partial JSON arguments before they are complete. cawir surfaces those deltas as events, but it waits for the final provider response before executing tools.

That keeps tool execution simple and safe:

```text
provider streams partial text/tool JSON
provider parser builds final MessageContent blocks
agent appends assistant tool_use blocks
tool registry executes complete tool input
agent appends user tool_result blocks
```

Executing from partial JSON would need cancellation, replacement, and validation rules that checkpoint 10 does not need.

## Provider-specific streams map to one event vocabulary

Anthropic and OpenAI stream different event shapes:

```text
Anthropic: content_block_delta / input_json_delta
OpenAI chat: choices[].delta.content / choices[].delta.tool_calls
OpenAI Responses: response.output_text.delta / response.output_item.done
```

Each provider owns its wire-format accumulator and maps into the same provider-neutral event shape. The agent then maps provider-neutral events into `AgentEvent`:

```text
ProviderEvent::TextDelta -> AgentEvent::AssistantTextDelta
ProviderEvent::ToolUseStart -> AgentEvent::AssistantToolUseStart
ProviderEvent::ToolUseInputDelta -> AgentEvent::AssistantToolUseInputDelta
```

That keeps surfaces and hooks from learning every provider's streaming protocol.

## Rendering needs state

Non-streaming rendering can print one line per final event:

```text
anthropic: complete answer
```

Streaming rendering needs to remember whether an assistant line is already open:

```text
AssistantTextDelta("hel") -> print "anthropic: hel" without newline
AssistantTextDelta("lo")  -> append "lo"
AssistantText("hello")    -> print only the newline
```

That final `AssistantText` event is still useful to non-terminal surfaces and hooks, but the REPL must suppress duplicate text after it already rendered the deltas.
