# Permission modes and plan mode

Checkpoint 5 moved cawir from inline approval prompts toward explicit permission policy.

The four modes are intentionally coarse:

```text
default       read allowed, write asks, shell asks
plan          read allowed, write denied, shell denied
accept-edits  read allowed, write allowed, shell asks
bypass        read allowed, write allowed, shell allowed
```

`Bypass` is dangerous because shell access is arbitrary local code execution. cawir still keeps a small catastrophic shell-command guard that applies even in bypass, but that guard is only a safety rail. It is not a sandbox and should not be treated as complete shell security.

## Default vs plan

`Default` means the agent may try to act, but the user approves risky actions one by one.

`Plan` means the agent may inspect and propose, but it may not mutate. Mutating tool calls become denied tool results instead of approval prompts.

That distinction matters for user intent:

```text
default: "You can work, but ask before changing anything risky."
plan:    "Do not change anything yet. Bring me a plan first."
```

## `exit_plan_mode` as a control-flow tool

`exit_plan_mode` is advertised as a tool because the model needs a structured way to say:

```text
I am done planning.
Here is the plan.
Ask the user to approve it.
```

A tool call gives cawir structured JSON:

```json
{ "plan": "1. Inspect...\n2. Edit...\n3. Test..." }
```

That is more reliable than trying to infer from free text whether a plan is ready.

But `exit_plan_mode` is not a normal execution tool. `read_file`, `write_file`, and `shell` act on the outside world. `exit_plan_mode` changes agent control flow and asks the human for a decision.

Current checkpoint shape:

```text
tools.rs parses exit_plan_mode
agent.rs returns TurnOutcome::PlanReady
repl.rs renders the plan and asks for approval
```

This keeps terminal UI out of tool execution. Tools should not own REPL prompts or mode switching.

Later, once registries exist, `exit_plan_mode` can be modeled as a control-flow tool or agent action rather than living beside filesystem/process tools forever.

## Plain-text fallback

Models do not always call the tool we want. In plan mode, OpenAI returned a plain text plan instead of calling `exit_plan_mode`, which left the user with no approve/reject prompt.

cawir now treats plain text final answers in plan mode as proposed plans too.

There are two paths:

```text
tool-backed plan:
  model calls exit_plan_mode
  REPL asks for approval
  approval/denial is sent back as a tool_result
  agent loop can continue

plain-text plan:
  model returns text
  REPL asks for approval
  approval switches mode back to default
  control returns to the prompt because there is no pending tool_use to answer
```

The tool-backed path is the cleaner target behavior. The plain-text path is a practical fallback for model reliability.
