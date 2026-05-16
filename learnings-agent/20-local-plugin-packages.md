# Local Plugin Packages

Checkpoint 12 made plugins a structured local capability source. A plugin is a directory with a `cawir-plugin.json` manifest, discovered from project settings:

```json
{
  "plugins": {
    "directories": ["plugins"]
  }
}
```

Each configured directory can either be a plugin root itself or a parent directory containing plugin roots.

## What plugins can contribute now

Plugins can add:

- slash commands, registered into the same `CommandRegistry` as built-ins
- command-backed tools, registered into the same `ToolRegistry` as built-ins and MCP tools
- hooks, appended into the normal hook settings
- settings snippets, deep-merged into the resolved project settings before MCP and hooks load

This keeps plugins as a source of existing extension types, not a parallel execution system.

## Why plugin tools are external

Plugin tools are `ToolKind::External` because cawir does not own their implementation. A plugin tool runs a manifest-declared process, which can inspect files, call networks, mutate state, or do anything the local command can do.

Treating plugin tools like MCP tools keeps the policy model consistent:

```text
built-in read-only tools: allowed in normal modes
built-in mutating tools: mode-specific approval or denial
external tools: approval-gated outside bypass, denied in plan mode
```

That is a trust-boundary decision, not just an implementation detail. The source of the tool matters when deciding whether the user should approve it.

## Hooks append, settings merge

Hooks are event subscribers. If project settings already define a `pre_tool_use` hook and a plugin contributes another, both should run. Appending preserves existing behavior:

```text
project hooks + plugin hooks = both handlers run
```

Settings snippets are configuration objects. Deep-merging lets a plugin provide defaults or nested values without deleting unrelated project settings:

```text
project settings object + plugin settings object = combined object
```

Arrays and objects have different meanings here. Hook arrays represent ordered subscribers, so appending is the least surprising behavior. Settings objects represent configuration state, so recursive object merge is the useful behavior.

## Tool names are namespaced

Plugin tools are registered as:

```text
plugin__<plugin_name>__<tool_name>
```

That mirrors MCP namespacing and avoids collisions with built-ins. The model sees these names in the provider tool array, so deterministic discovery and ordering still matter for prompt-cache stability.

Plugin tool registration sorts by the final registered name before adding tools to the runtime registry. That means a manifest reordering that leaves names, descriptions, and schemas unchanged does not change the provider-facing tool array order.

## Command-backed tools

A plugin tool runs its manifest `command` through `sh -c` from the project root. cawir sends the tool input JSON on stdin and captures stdout as the tool result. Nonzero exits become error tool results instead of aborting the agent turn.

This process contract is deliberately small:

```text
stdin  = structured input JSON
stdout = command result
stderr/nonzero exit = tool error content
env    = execution context
```

cawir sets these environment variables for plugin commands and tools:

```text
CAWIR_PLUGIN_DIR
CAWIR_PLUGIN_NAME
CAWIR_PROJECT_ROOT
```

Slash commands also receive raw user arguments in `CAWIR_COMMAND_ARGS`.

The important agent-design choice is that plugin tools still pass through the normal permission, hook, event, and tool-result path. External tools are approval-gated outside bypass mode and denied in plan mode, just like MCP tools.

Using stdin and environment variables avoids creating a Rust plugin ABI, dynamic library loading story, embedded scripting runtime, or plugin SDK before the project has real pressure for one. It also keeps plugins language-agnostic: any executable that can read stdin and write stdout can participate.
