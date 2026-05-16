# Runtime Plugin Contributions

Checkpoint 12 added plugin-loaded commands and tools. That reused a Rust pattern that already appeared with MCP: trait methods should return borrowed `&str` when names can come from runtime-owned `String` values.

Built-in slash commands used to fit this shape:

```rust
fn name(&self) -> &'static str {
    "/help"
}
```

`'static` means the string lives for the whole program. That works for string literals compiled into the binary, but a plugin command name comes from JSON:

```json
{ "name": "/hello" }
```

The plugin command stores that as an owned `String`, so the command trait changed to:

```rust
fn name(&self) -> &str;
```

This is more flexible without making callers own or clone the name. Built-ins still return string literals, and plugin commands return `&self.command.name`.

The key Rust idea is that `&str` says only "borrowed string slice." It does not say where the backing storage lives. That backing storage can be:

- a string literal with `'static` lifetime
- an owned `String` field inside a registered plugin command
- another string-like value that lives at least as long as the borrow

So `&str` is the right trait return type when implementations have different storage strategies but callers only need to compare or display the name.

## Why plugin structs own paths and strings

Plugin commands and tools store `PathBuf` and `String` values because the manifest file is read during startup, then the parsed JSON buffer is dropped. Anything used later by a slash command or tool must own its data.

That is the same ownership rule as provider and MCP runtime data:

```text
startup parsing can borrow temporarily
registered runtime objects must own what they need later
```

The registry stores `Box<dyn Command>` and `Box<dyn Tool>` trait objects. Those objects may live for the whole REPL session, so they cannot borrow from a temporary parsed manifest value. Owning the data avoids lifetime parameters on the registry and keeps the runtime object boundary simple.

## `serde` default fields

The manifest uses `#[serde(default)]` so older or smaller manifests can omit optional sections:

```rust
#[serde(default)]
commands: Vec<RawPluginCommand>,
#[serde(default)]
tools: Vec<RawPluginTool>,
```

An absent `commands` key becomes an empty vector. That keeps the manifest format extensible without adding manual `Option<Vec<_>>` handling everywhere.

## Sorting runtime contributions

Plugin tools are sorted after they are converted into runtime `PluginCommandTool` values:

```rust
tools.sort_by(|left, right| left.name().cmp(right.name()));
```

Sorting after conversion matters because the final registered name includes normalization and namespacing:

```text
plugin__<plugin_name>__<tool_name>
```

That is the value the provider sees in the tool array. Sorting on the final name is therefore more direct than sorting on raw manifest fields.
