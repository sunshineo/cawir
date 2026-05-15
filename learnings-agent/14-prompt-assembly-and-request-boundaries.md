# Prompt assembly and request boundaries

## Three different inputs

An agent model request has at least three conceptually different inputs:

- **Prompt**: per-request operating context, such as identity, behavior, environment, and project guidance.
- **History**: durable conversation state, such as user messages, assistant messages, tool calls, and tool results.
- **Tools**: structured callable capability definitions, such as names, descriptions, and JSON schemas.

These should not be flattened into one giant text prompt. The provider layer should receive them separately and serialize them into each provider's native wire format.

Examples:

- Anthropic: `system`, `messages`, and `tools`.
- OpenAI chat completions: a system message plus `messages` and `tools`.
- OpenAI Responses/Codex: `instructions`, `input`, and `tools`.
- Ollama: a system message plus chat messages and native tools.

## Project memory belongs in prompt context

`AGENTS.md` / `CLAUDE.md` guidance is not durable conversation history. It is project context that should be rebuilt before the model call.

That keeps sessions clean:

- saved sessions contain what actually happened in the conversation
- prompt assembly can change without rewriting old sessions
- project guidance updates naturally affect the next request

## Tool schemas stay out of prompt text

Tool schemas should travel through provider-native structured tool fields, not be pasted into prompt text. This keeps instructions readable and lets providers validate tool calls using their tool APIs.

This also matters for caching. Anthropic includes structured tools in the cached prefix, so tool definitions and ordering still affect cache hits. The answer is not to inline tools into prompt text; the answer is to keep tool definitions deterministic and make cache behavior observable.

## Cache policy after the Anthropic audit

8.5d created the prompt assembly layer without adding block-level Anthropic cache breakpoints. That was deliberate: cache markers are provider-specific policy, and the request shape needed to be checked against Anthropic's current docs before cawir encoded a strategy.

8.5h made the policy concrete for Anthropic:

- Keep top-level automatic `cache_control` so the cache breakpoint moves forward with the growing conversation.
- Also put an explicit `cache_control` breakpoint on cawir's single system text block. Because Anthropic cache prefixes are ordered `tools` → `system` → `messages`, that system breakpoint covers stable tool definitions plus the assembled identity/behavior/environment/project-guidance prompt.
- Do not add thinking-clearing headers or mutate prior reasoning/context as a cache optimization. Anthropic's April 2026 Claude Code postmortem showed how a repeated `clear_thinking_20251015` request flag caused degraded behavior and cache misses.

The resulting request has two cache layers: a stable project/tool/system prefix, and an automatic conversation prefix. This is slightly more explicit than automatic-only caching, but it keeps the provider-specific choice isolated inside `anthropic.rs`.

## Why cawir does not use every cache breakpoint yet

Claude Code can use multiple cache breakpoints because it has multiple large context layers with different stability profiles. cawir does not yet have subagents, compaction summaries, memory extraction, dynamic MCP context, plugin context, or skill-selected context.

So 8.5h uses only the breakpoints that match cawir's current request shape:

```text
tools + system/project guidance  -> explicit system-block breakpoint
conversation history             -> top-level automatic moving breakpoint
newest turn                      -> fresh uncached tail
```

Adding more breakpoints before more context layers exist would be cargo-culting Claude Code's shape rather than learning from it. The right time to add another breakpoint is when cawir adds a new request layer that is large, stable across turns, and invalidates differently from the current tools/system/history layers.
