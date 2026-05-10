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

## Cache policy waits for the provider audit

8.5d creates the prompt assembly layer, but it deliberately does not add block-level Anthropic cache breakpoints.

Explicit cache markers are provider-specific policy. They require deciding which exact request blocks are stable, how tool schemas interact with the cached prefix, and whether top-level automatic caching is enough for cawir's agent loop. That belongs in 8.5h, the Anthropic prompt-cache request audit.
