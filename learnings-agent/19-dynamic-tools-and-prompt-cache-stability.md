# Dynamic Tools And Prompt Cache Stability

Provider-side prompt caches are sensitive to the exact request prefix. For Anthropic tool use, the hierarchy is `tools` -> `system` -> `messages`, so changing the advertised tool array can invalidate the cached system and conversation prefix that follows it.

MCP makes the tool array dynamic because tools are discovered from external server processes at startup or resume. That means cache stability depends on deterministic registration:

- Start from built-ins in a stable order.
- Register MCP tools with stable namespaced names.
- Sort discovered MCP tools by registered name before adding them to the registry.
- Keep tool descriptions and schemas stable unless the tool surface intentionally changed.

cawir records a fingerprint of the exact provider tool-definition array in each saved session. On resume, it recomputes the current fingerprint and warns when it differs from the saved value, because the next model request may rebuild prompt cache.

This is observability, not policy. A fingerprint mismatch does not mean the session is broken; it means the cache key likely changed. The user can still continue, but they get a clear explanation for possible `cache_create` spikes or lower `cache_read` counts.

## Why MCP gets special care

Built-in tools are compiled into cawir, so their names, descriptions, and schemas change only when cawir changes. MCP tools come from external server processes. A server can be missing, start slowly, return tools in a different order, or upgrade its tool schema independently of cawir.

That makes MCP a cache-risk boundary:

```text
same session + different MCP tool array = likely different provider cache key
```

The Claude Code cache regression in early 2026 was a useful warning sign. Public reports showed resume flows rebuilding prompt cache when deferred tools, MCP tools, or custom agents reconstructed a different tool/schema state than the original session. The underlying lesson applies to cawir even though the implementation is different: dynamic capability discovery must be observable and deterministic.

## What cawir records

cawir fingerprints the exact provider-facing tool definition array, not just MCP config. This is deliberate. The provider sees names, descriptions, schemas, and ordering. A config hash would miss server-side schema changes; a provider-array hash catches what actually matters for cache behavior.

The fingerprint is stored with the session:

```text
tool_definition_fingerprint: "fnv1a64:..."
```

On resume, cawir rebuilds the runtime tool registry from the current project settings and MCP servers, recomputes the fingerprint, and warns if it differs from the saved fingerprint.

## Why this is not a hard error

A changed tool fingerprint can be valid:

- The user intentionally added an MCP server.
- A server upgraded its schema.
- The current mode advertises a different tool set.
- A server is temporarily unavailable.

Blocking the session would be too strict. The right behavior for now is to warn, continue, and print the current tool fingerprint beside usage and cache counters so cache behavior is explainable after the request.
