# Hooks and settings

Checkpoint 9 turned the event vocabulary from passive progress data into a synchronous policy extension point.

## Settings load order

cawir now loads settings in this order:

```text
~/.claude/settings.json
<project>/.claude/settings.json
<project>/.claude/settings.local.json
```

Later files override earlier files, with recursive object merging. This gives three scopes:

- user defaults that apply everywhere
- project settings that can be committed
- local project settings that can stay machine-specific

The outer settings file stays open-ended JSON for now. That keeps checkpoint 9 from inventing typed structures for settings that do not exist yet. Each consumer should parse only the section it owns; hooks parse `hooks`, future plugin or skill code can parse its own section later.

## Hooks consume the same events surfaces render

Hooks receive serialized `AgentEvent` JSON. That keeps one lifecycle vocabulary:

```text
agent loop emits AgentEvent
hook registry consumes AgentEvent synchronously
REPL renders AgentEvent for the user
```

The important difference is timing. Hook dispatch happens before the event is rendered, because hooks may deny or modify the work. The REPL only observes the result of that decision.

## Pre-tool hooks are decision points

`PreToolUse` is the first useful hook point for tool policy:

```text
model requested tool
run pre-tool hooks
maybe modify input
maybe deny
prepare and execute tool
emit post-tool summary
```

A denial becomes an error `tool_result`, not a process-level crash. That keeps the model loop recoverable: the model can see the denial and choose another path.

This matches the existing tool-failure rule. Missing files, bad input, user denials, and hook denials are all model-visible tool results. The REPL only aborts the turn for process-level failures such as provider errors or prompt assembly failures.

## Post-tool hooks are side-effect observers

`PostToolUse` runs after a tool finishes and carries the original input plus summary metadata. That makes stateless handlers possible. For example, a post-write hook can check:

```text
event.type == "post_tool_use"
event.name == "write_file"
event.input.path ends with ".rs"
```

Then it can run `cargo fmt` without remembering anything from the earlier pre-tool event.

## Command hook protocol

The first hook implementation is intentionally small:

- event JSON goes to stdin
- exit status nonzero means deny
- empty stdout on success means allow
- JSON stdout can say allow, deny, or modify input

That is enough to demonstrate configured hooks without adding plugin loading, embedded scripting, or subagent handlers before they have real pressure.

Because the boundary is stdin/stdout, the hook implementation language is not part of cawir's Rust API. A command hook can be shell, Python, Node, Rust, or any other executable that follows the small JSON action contract.
