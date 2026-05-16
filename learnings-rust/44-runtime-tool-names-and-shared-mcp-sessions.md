# Runtime tool names and shared MCP sessions

Checkpoint 11 made the tool registry hold both built-in tools and MCP-discovered tools. That changed two Rust shapes: tool names can now be runtime data, and multiple registered tools can share one server connection.

## `&'static str` vs `&str`

Before MCP, every tool name was a string literal:

```rust
fn name(&self) -> &'static str {
    "read_file"
}
```

`&'static str` means the string data lives for the whole program. String literals are baked into the binary, so this was a natural fit for built-ins.

MCP tool names are discovered at runtime:

```text
server "github" exposes tool "list_issues"
registered name becomes "mcp__github__list_issues"
```

That registered name is an owned `String` stored inside `McpTool`. The trait changed to:

```rust
fn name(&self) -> &str;
```

This says callers only need a borrowed string slice. Built-ins can still return string literals, and MCP tools can return `&self.registered_name`. The caller does not care where the string came from.

General rule:

```text
Use &'static str when the value is truly fixed for the program lifetime.
Use &str in trait methods when implementations may borrow from owned String data.
```

## Why `Arc<Mutex<McpServerSession>>`

One MCP server process can expose many tools. Each registered MCP tool needs to call the same server process over the same stdin/stdout JSON-RPC connection.

The shape is:

```rust
Arc<Mutex<McpServerSession>>
```

`Arc<T>` gives shared ownership. Each `McpTool` can hold a clone of the pointer without cloning the server session or starting another process.

`Mutex<T>` gives exclusive access while a tool call is using the session. That matters because a stdio MCP connection has one stdin stream, one stdout stream, and request IDs. Interleaving writes and reads from multiple tool calls without coordination would make the protocol harder to reason about.

This is similar to:

```text
Arc      = many handles own the same session
Mutex    = one handle uses the session at a time
Session  = child process + JSON-RPC state
```

## Why not one process per tool

Starting one MCP server per tool would waste work and could break server state. Many MCP servers are designed as long-running processes that expose a tool set after `initialize` and `tools/list`. Keeping one session per configured server matches the protocol better:

```text
server process starts once
tools/list discovers all tools
each registered tool calls tools/call on the same session
session drops when the registry is dropped or reloaded
```

## Why the mutex is acceptable here

The current tool execution path is synchronous from the registry's point of view. A tool call blocks until the MCP server returns a result. The mutex is not hiding parallel work; it is documenting that the connection is a single shared resource.

If cawir later supports parallel tool calls, the same lock keeps stdio MCP sessions safe while leaving room for other tools or other MCP servers to run independently.
