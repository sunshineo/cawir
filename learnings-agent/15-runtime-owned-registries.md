# Runtime-owned registries

Checkpoint 8.5e moved tool registry ownership into `Runtime`.

## Runtime state vs session state

`Runtime` holds live executable state:

```text
HTTP client
active provider object
credential
command registry
tool registry
```

`Session` holds durable conversation data:

```text
session id
provider name
model name
permission mode
project path
message history
```

A `ToolRegistry` contains actual Rust implementations such as `ReadFileTool` and `ShellTool`. Those are executable capabilities, not conversation data, and they cannot be meaningfully serialized into session JSON.

When a session is resumed, cawir should reconstruct the available capabilities from startup state: built-ins now, later MCP servers, plugins, or skills. It should not load executable behavior from old session history.

## One registry per running runtime

Before 8.5e, `tools.rs` rebuilt built-ins internally when the agent needed tool definitions or executed tool calls. That worked while every tool was hardcoded, but it hid the ownership boundary.

After 8.5e, the flow is explicit:

```text
REPL creates Runtime
Runtime owns ToolRegistry
agent::TurnContext borrows &ToolRegistry
provider request gets definitions from that registry
tool execution dispatches through that same registry
```

The agent loop no longer decides how the registry is populated. It only receives the registry it should use for this runtime.

That matters for later checkpoints. Hooks, MCP, plugins, and skills can add population sources without making the core loop rebuild or rediscover tools on every request.

## Deterministic tool order is behavior

Tool order is not just cosmetic once provider prompt caching exists.

Providers can include structured tool definitions in the cacheable request prefix. These two equivalent-looking tool sets may produce different cache prefixes:

```text
list_files, read_file, write_file
read_file, list_files, write_file
```

If the prefix changes, the provider may miss the prompt cache. That can increase latency, cost, and make cache behavior harder to reason about.

cawir keeps built-ins in a `Vec`, which preserves insertion order, and tests now assert the advertised order for default and plan modes. Mode-specific availability is therefore explicit:

```text
default mode: normal built-in tools
plan mode: normal built-in tools plus exit_plan_mode
```

The general agent-design lesson: provider-facing capability lists are part of the request contract. If they affect caching or model behavior, make their order deterministic and test it.
