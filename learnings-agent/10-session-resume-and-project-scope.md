# Session resume and project scope

Checkpoint 7 made conversations durable. The first version restored model context, but the UX exposed several agent-design rules.

## Restoring context is not the same as rendering context

When cawir loads a session, the model history is restored internally. That alone is not enough for a human using the terminal.

If the transcript is hidden, the next prompt is confusing:

```text
cawir> 
```

The model has old context, but the user cannot see what context is active. Resume should make hidden model state visible enough for the human to orient.

cawir now prints the previous conversation after resume. Text blocks are printed directly; tool calls and tool results are summarized so large file reads or shell output do not flood the terminal.

## A session id is not the same as a useful conversation

The first persistence pass saved a session at startup. That created empty session files:

```text
session id exists
messages = []
```

Those files are technically sessions, but they are not useful resume targets. The product rule became:

```text
resumable session = session with at least one message
```

This rule affects both `/resume` listing and `--continue`. A resume UI should list useful conversations, not implementation artifacts.

## Save rules are product rules

"Save on every state change" is simple and robust, but it can create clutter. "Save only when exiting" risks losing work. The current compromise is:

```text
do not write brand-new empty sessions
save sessions once they contain messages
always save sessions loaded from disk
```

That keeps automatic persistence while avoiding a pile of empty startup sessions.

## Project-scoped lists by default

Sessions are stored in an OS-level cawir data directory, not inside each repository. Without project metadata, `/resume` can show unrelated conversations from other projects.

Adding `project_path` lets cawir default to project-scoped resume:

```text
/resume       list sessions for this project
--continue    resume latest non-empty session for this project
--resume id   exact id escape hatch
/resume id    exact id escape hatch inside the REPL
```

The exact-id path remains global on purpose. It is useful for recovery, debugging, and old sessions that do not yet have project metadata.

## Current active session should not be offered as a target

Once `/resume` could list sessions, it initially included the session currently running in the REPL. That is not useful. A resume list should show other possible targets, not the state the user is already in.

The list now excludes the active session and empty sessions.
