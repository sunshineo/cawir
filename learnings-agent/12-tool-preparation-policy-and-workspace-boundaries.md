# Tool preparation, policy, and workspace boundaries

Checkpoint 8.5a split tool execution into three conceptual phases:

```text
prepare
policy / approval
execute
```

This keeps tools focused on their domain while moving cross-cutting decisions out of individual tool bodies.

## What tools should own

A tool should understand its own input and intended effect:

```text
read_file:
  parse path
  resolve path inside the workspace
  prepare a read action

write_file:
  parse path and content
  resolve writable path inside the workspace
  prepare a write action and approval summary

shell:
  parse command
  reject hard-coded catastrophic commands
  prepare a shell action and approval summary
```

The central executor should not know that `write_file` has `content` or that `shell` has `command`. If it did, the executor would become a second implementation of every tool.

## What policy should own

Policy should evaluate generic prepared metadata:

```text
tool kind
approval request
workspace-valid prepared effects
current permission mode
```

The current permission matrix is:

```text
Plan:
  read-only allowed
  mutating denied
  exit_plan_mode allowed

Default:
  read-only allowed
  write/shell ask user

AcceptEdits:
  read-only allowed
  write allowed
  shell asks user

Bypass:
  read-only allowed
  write/shell allowed
```

The important distinction is:

```text
workspace boundary:
  can this cawir run touch this path at all?

permission mode:
  if the action is in scope, does it need approval right now?
```

`Bypass` skips approval prompts for in-scope actions. It should not silently widen the workspace boundary.

## What the surface should own

Approval is UI. The terminal REPL asks:

```text
write_file wants to write 1200 bytes to src/main.rs
approve? [y/N]
```

A future TUI, JSON protocol, or web surface will ask differently. That is why approval now flows through a callback instead of being printed directly inside `write_file` or `shell`.

## Outside-project access

For now, outside-project file access is denied in every mode. That is safer than treating `Bypass` as full filesystem access.

If cawir later needs outside-project access, it should be explicit, for example:

```toml
[workspace]
extra_roots = [
  "/tmp/cawir-fixtures"
]
```

or a one-turn approval prompt for a specific external path.

That keeps two powers separate:

```text
skip approval prompts
expand the workspace
```

Combining them into one mode would make `Bypass` too broad.
