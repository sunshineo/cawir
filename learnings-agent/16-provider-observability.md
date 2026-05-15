# Provider observability

Checkpoint 8.5f made model-call results more observable without changing the session transcript format.

Provider output now has two parts:

- the conversational result: assistant text or tool-use blocks
- request metadata: token usage and provider diagnostics

The session still stores only durable conversation data. Token counts are request diagnostics, not prompt history, so they are emitted as `AgentEvent::ModelRequestFinish` and rendered by the REPL instead of being saved as messages.

Rate limits also became provider-neutral. Concrete providers still know the HTTP response shape, but they all call a shared helper that turns HTTP 429 into a typed rate-limit error and preserves `retry-after` when present. Later retry policy can match that error variant instead of scraping strings.

Anthropic cache counts are especially important for agent loops. A cache miss can come from a short prompt, unstable system/project instructions, changing tool definitions, or a provider request-shape bug. Surfacing `cache_create` and `cache_read` gives us a way to notice those failures before building hooks, MCP, or plugins on top of the provider boundary.

