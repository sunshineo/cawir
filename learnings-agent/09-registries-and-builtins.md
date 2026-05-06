# Registries and built-ins

Checkpoint 6 changed tools and slash commands from hardcoded dispatch matches into registries.

Before the registry, dispatch looked like:

```text
if tool name is read_file, call read_file
if tool name is shell, call shell
otherwise unknown tool
```

That works early because there are only a few capabilities. It starts to strain once each capability has metadata, policy classification, availability rules, and execution logic.

The registry shape keeps those pieces together:

```text
tool:
  name
  description
  input schema
  read-only/mutating/control-flow classification
  mode availability
  execution
```

That matters for agents because the model sees the advertised tool schema, but the runtime enforces the actual behavior. If those live far apart, they can drift: the model may be told a tool exists while the executor handles it differently.

## Registry vs match

The old match style was fine while tools were just a few function calls:

```text
match tool name:
  read_file  -> execute_read_file
  shell      -> execute_shell
```

Checkpoint 6 moved away from that because the tool boundary now has more than execution. A tool also has a provider-visible schema, a human-readable description, a policy classification, and mode-specific availability.

The registry makes the dispatch rule boring:

```text
find the capability by name
run the common interface
```

That leaves each capability responsible for its own details.

## Built-ins first

cawir now has registries, but the registries are still populated directly in Rust:

```text
builtins:
  read_file
  list_files
  write_file
  shell
  exit_plan_mode
```

This is deliberate. A registry is a dispatch seam, not a plugin system by itself.

External population waits until later checkpoints:

```text
MCP      external tool servers
plugins  packaged extensions
skills   higher-level workflows
```

The useful intermediate state is:

```text
hardcoded capabilities
registry-shaped dispatch
no external loading yet
```

That preserves readability while making the next extraction smaller.

## Commands are tools for humans, not models

Slash commands and model tools both use registries, but they serve different callers.

Model tools are advertised to the provider and return `tool_result` blocks into conversation history.

Slash commands are local REPL controls. They mutate local state such as provider, model, and permission mode, but they are not sent to the model as user messages.

Keeping separate registries avoids mixing two different protocols:

```text
model tool protocol: provider-visible, JSON-shaped, history-backed
slash command protocol: local terminal control, human-visible, state-backed
```
