# cawir backlog

This file holds concrete follow-ups that are worth keeping but are not part of the
main checkpoint sequence yet. Promote an item into [`roadmap.md`](roadmap.md) only
when we decide to actively work it as a checkpoint or sub-step.

## Tool permission and approval backlog

Current state: cawir has a small, explicit permission model. `PermissionMode`
currently has `default`, `plan`, `accept-edits`, and `bypass`; tools declare a
coarse `ToolKind`; policy returns allow, ask, or deny; hooks can deny or modify
pre-tool input; and each surface turns approval requests into a boolean
approved/denied answer. This is enough to keep mutating actions gated, but it is
still a per-call prompt system rather than a useful permission system.

The next permission checkpoint should add remembered grants and clearer policy
state without adding an LLM-based danger classifier. An auto-approve classifier
can stay out of scope until there is real pressure for it.

### Recommended order

1. Add session-scoped permission grants.
2. Expand approval responses beyond a boolean.
3. Add rule matching for tool name, kind, path, command, and external source.
4. Add a dedicated read-only or inspect mode separate from plan mode.
5. Make dangerous bypass explicit and noisy.
6. Add user/project policy defaults through settings.
7. Improve approval detail shown by REPL, TUI, App Server, and exec.
8. Add audit events and regression tests for permission decisions.

### Session-scoped permission grants

Let the user approve a class of future tool calls for the current session instead
of answering the same question repeatedly.

Examples:

- allow `edit_file` for the rest of this session
- allow writes under `docs/` for this session
- allow `shell` command `cargo test` for this session
- allow a specific MCP or plugin tool for this session
- deny a specific tool for this session

The first version should be session-scoped only, stored in `Session`, and visible
on resume. Permanent user or project policy can come later after the matching
rules are stable.

Rust topics: serializable grant records, matching prepared tool calls, session
schema evolution, deterministic policy evaluation.

### Rich approval responses

Change approval from a boolean into an explicit decision enum.

Possible decisions:

- allow once
- deny once
- allow matching calls for this session
- deny matching calls for this session
- allow all tools in this mode for this session

Surfaces can start simple. The REPL can accept short keys, TUI can show buttons,
App Server can expose structured decisions, and `exec` can keep deterministic
flags such as deny-by-default or approve-all.

Rust topics: protocol-compatible enum evolution, defaulting old boolean clients,
surface adapters around one core decision type.

### Rule matching and permission scope

Define a small permission-rule language over prepared tool calls.

Useful match fields:

- tool name: `edit_file`, `shell`, `mcp__github__list_issues`
- tool kind: read-only, write-file, shell, external
- path prefix or suffix for file tools
- shell command exact match or conservative prefix match
- external source, such as MCP server name or plugin name

Rules should match prepared input after path normalization, not raw model input,
so `docs/../src/lib.rs` and `src/lib.rs` evaluate consistently.

Rust topics: rule enums, pattern matching over prepared input, canonical paths,
careful string matching for shell commands.

### Read-only or inspect mode

Add a mode that allows inspection but blocks mutation without the planning
approval workflow.

`plan` currently allows read-only tools and blocks writes/shell/external tools,
but it also has special plan-ready behavior. A separate mode such as `read-only`
or `inspect` would be useful when the user wants safe analysis only, not a
proposed implementation plan.

The mode matrix should stay understandable:

- `default`: read allowed, writes/shell/external ask
- `read-only` or `inspect`: read allowed, mutation and external tools denied
- `plan`: read allowed, mutation denied, plan approval flow enabled
- `accept-edits`: reads and file edits allowed, shell/external ask
- `bypass` or `dangerously-skip-permissions`: everything allowed except hard
  catastrophic guards

Rust topics: enum expansion with serde compatibility, command parsing aliases,
mode-specific tool availability tests.

### Dangerous bypass semantics

Keep the current bypass capability, but make the danger explicit in naming,
display, and startup behavior.

Follow-ups:

- add an alias or command spelling such as `dangerously-skip-permissions`
- print a clear warning when entering the mode
- keep the catastrophic shell guard even in bypass
- make bypass state obvious in REPL/TUI/App Server session metadata
- consider requiring explicit user action to persist or resume a bypass session

This is not a sandbox. The docs and UI should say that plainly.

Rust topics: command aliases, warning events, session metadata, conservative
resume behavior.

### User and project policy defaults

After session grants work, add settings-backed defaults.

Examples:

- always allow read-only built-ins
- allow `cargo test` and `cargo fmt` shell commands in this project
- always ask before shell
- deny external plugin tools unless explicitly approved
- deny writes outside specific project subdirectories

Project policy should be cawir-owned settings, with local overrides for personal
preferences. It should not rely only on hooks, because hooks are executable code
and permission defaults should be inspectable as data.

Rust topics: settings parsing, declarative policy structs, merge precedence,
validation with helpful errors.

### Approval detail and previews

Approval prompts need enough context for the user to make a decision.

Follow-ups:

- show file path and byte counts for writes
- show old/new text lengths and maybe a small diff preview for edits
- show shell command, working directory, and timeout
- show MCP server or plugin name for external tools
- include the normalized path used for policy checks

The core approval request should carry structured detail. Each surface can render
that detail differently.

Rust topics: richer `ToolApprovalRequest`, serializable approval params,
surface-specific rendering.

### Audit events and tests

Permission decisions should be observable.

Add events or metadata that show why a tool was allowed, denied, or sent for
approval:

- mode decision
- session grant match
- project policy match
- hook denial
- user approval decision
- bypass decision

Tests should cover the policy matrix, session grant matching, path normalization
before matching, bypass warnings, old boolean App Server clients, and denied tools
becoming tool-result errors instead of aborting the turn.

Rust topics: event payload design, fake approval callbacks, table-driven policy
tests.

## Agent reliability, review, and evaluation backlog

Current state: cawir can run turns, call built-in tools, edit files, execute shell
commands, persist sessions, and surface typed provider failures. That is enough to
learn the core loop, but it is still weak in the places that make a real coding
agent dependable: showing exactly what changed, recovering from failed turns,
debugging provider/tool behavior, handling transient failures, and proving common
agent workflows keep working.

These items are intentionally separate from prompt, context, and permission work.
They are the "can I trust and debug this agent?" layer.

### Recommended order

1. Add turn-level change summaries.
2. Add git-aware workspace safety checks.
3. Harden tool registry and external-tool lifecycle behavior.
4. Add provider retry, timeout, and cancellation policy.
5. Add redacted request logging and turn replay.
6. Add offline behavior evaluation fixtures.
7. Add failure recovery and rollback ergonomics.
8. Centralize secret and log redaction.

### Turn-level change summaries

Track the observable side effects of each assistant turn and show a compact summary
before returning control to the user.

Useful first version:

- files created, edited, or deleted
- shell commands run and their exit status
- tests or checks run
- approval decisions made during the turn
- provider request count and token usage, when available

This is not a replacement for `git diff`. It is a lightweight turn ledger that
helps the user understand what the agent just did without digging through the full
event stream.

Rust topics: event aggregation, separating durable session history from debug
metadata, compact display structs.

### Git-aware workspace safety

Add a small layer that understands the current git state before and after mutating
actions.

Follow-ups:

- warn before edits when the working tree is already dirty
- distinguish files changed by the agent from unrelated pre-existing changes
- expose `/status` and `/diff` commands in the REPL
- optionally help create a branch or worktree before larger changes
- never auto-reset or auto-revert user changes

The first version can shell out to `git` instead of using a Rust git library. That
keeps the behavior easy to inspect while the project is still a learning tool.

Rust topics: process output parsing, fallible optional features when a directory is
not a git repo, path normalization.

### Tool registry and external-tool lifecycle hardening

The tool registry should eventually treat built-ins, skills, MCP tools, and future
plugins as one typed inventory with clear availability and failure behavior.

Follow-ups:

- stable tool ids and namespaces to avoid name collisions
- schema validation before tools are exposed to the model
- startup diagnostics for unavailable MCP servers or broken tool definitions
- per-tool timeouts and cancellation hooks
- clear distinction between model-visible tool descriptions and internal executor
  metadata
- tests that unavailable external tools degrade cleanly instead of breaking prompt
  assembly

This should stay data-driven. Tool descriptions, permission metadata, and executor
metadata are related, but they should not be mixed into one unstructured string.

Rust topics: registry structs, enum dispatch vs trait objects, serde schema
round-trips, namespacing.

### Provider retry, timeout, and cancellation policy

cawir already has typed provider errors and marks some stop failures as retryable,
but there is no real retry or cancellation policy yet.

Follow-ups:

- retry transient network failures and rate-limit responses with capped backoff
- do not retry after a mutating tool call unless the turn state is known to be safe
- add provider request timeouts
- add user cancellation for long model calls and long-running tools
- preserve partial event history when a turn is canceled
- record whether a failure was retryable, retried, exhausted, or canceled

This work should be conservative. Retrying model calls is usually safe before tool
execution, but retrying after file writes or shell commands can duplicate side
effects.

Rust topics: `tokio::time::timeout`, cancellation tokens or channels, retry loops,
idempotency flags.

### Redacted request logging and turn replay

Add an opt-in debug mode that writes enough structured information to reproduce or
inspect a turn without leaking credentials.

Useful debug artifacts:

- assembled system prompt sections and source labels
- model-visible tools and schemas
- conversation history metadata and token estimates
- provider request and response shape
- tool calls and tool results
- compaction decisions, when compaction exists

Pair this with a replay harness that can load a saved session and drive the agent
against a fake provider. The goal is to debug the agent loop itself without needing
live API calls.

Rust topics: serializable debug artifacts, redaction at serialization boundaries,
fake provider implementations.

### Offline behavior evaluation fixtures

Build a small evaluation harness for common coding-agent workflows.

Good early scenarios:

- read a file and answer a question
- edit one file with an exact expected diff
- run a check and summarize the failure
- request approval for a mutating tool
- deny a tool and continue gracefully
- compact or refuse a too-large request once context budgeting exists

Keep the scoring simple at first: expected tool sequence, final text contains,
files changed, and command results. This is enough to catch regressions while the
agent is still small.

Rust topics: temp directories, fake providers, deterministic fixtures,
table-driven tests.

### Failure recovery and rollback ergonomics

The runtime can remove a failed assistant message from session history, but file
system and shell side effects are outside that history.

Follow-ups:

- record which files were changed during a turn
- snapshot file contents before agent edits
- show recovery hints after a failed or canceled turn
- offer an explicit user-triggered restore for agent-edited files
- keep shell side effects as logged facts rather than pretending they can always be
  rolled back

Do not make automatic rollback the first step. It is safer to make side effects
visible, then add explicit recovery commands once the change ledger is trustworthy.

Rust topics: file snapshots, side-effect records, explicit command design.

### Secret and log redaction

Credential handling already avoids printing raw secret values, but future debug
logs, prompt previews, provider traces, MCP metadata, and App Server events will
increase the leak surface.

Follow-ups:

- centralize redaction for headers, environment variables, JSON fields, and tool
  inputs
- mark sensitive provider/auth fields in data structures
- test common secret-looking keys such as `api_key`, `authorization`, `token`, and
  `password`
- prefer allowlisted debug fields where practical
- make redacted output obvious without revealing length or value shape when that
  would leak useful information

Rust topics: recursive JSON redaction, wrapper types for sensitive strings,
snapshot tests for debug output.

## Prompt engineering and instruction customization backlog

Current state: cawir has the right structural seam for prompt assembly, but the
actual prompt is intentionally bare-bones. `prompt.rs` currently emits identity,
behavior, environment, project guidance, and active skills. Project guidance comes
from `AGENTS.md` / `CLAUDE.md` files in the project hierarchy, active skill bodies
are inserted only after a prompt match, and tool schemas stay in provider-native
tool fields instead of prompt text.

That is a good foundation for learning request boundaries, but it is not yet a
serious coding-agent prompt. Mature coding agents put substantial design into
their base operating prompt, tool-use policy, instruction precedence, project and
user customization, prompt inspection, and prompt-regression tests. cawir should
add those pieces deliberately without trying to clone private prompts from other
tools.

### Recommended order

1. Define cawir's instruction hierarchy and conflict rules.
2. Add user-level and cawir-owned project instruction sources.
3. Expand the base operating prompt into named, testable sections.
4. Add mode, permission, and workflow guidance to prompt assembly.
5. Review and improve built-in tool descriptions and schemas.
6. Add prompt-injection and data-vs-instruction boundaries.
7. Add prompt inspection and prompt snapshot tooling.
8. Add prompt-behavior regression scenarios for common coding-agent tasks.

### Instruction hierarchy and conflict rules

Define which instruction sources exist and which one wins when they conflict.

The first version should be explicit and small, for example:

- built-in cawir operating rules
- user-level cawir instructions
- project-level cawir instructions
- nested project guidance from `AGENTS.md` / `CLAUDE.md`
- active skill instructions
- direct user prompt for the current turn

The loader currently concatenates project guidance from ancestors to children, but
there is no documented precedence model, no conflict language in the prompt, and
no place for user-global preferences.

Rust topics: typed instruction sources, deterministic ordering, source labels,
tests for precedence and duplicate handling.

### User and project instruction sources

Add cawir-owned instruction files in addition to compatibility loading for
`AGENTS.md` / `CLAUDE.md`.

Possible sources:

- OS config directory user instructions, such as `instructions.md`
- project `.cawir/instructions.md`
- project `.cawir/instructions.local.md` for uncommitted local preferences
- existing `AGENTS.md` / `CLAUDE.md` compatibility files

Keep the current `AGENTS.md` / `CLAUDE.md` support, but make cawir's own files the
documented native path so this project is not implicitly shaped around another
agent's config format.

Rust topics: config path discovery, merge order, local-only files, source
metadata for rendered prompt sections.

### Base operating prompt

Replace the tiny behavior string with a set of named operating sections.

Candidate sections:

- role and scope: cawir is a small coding agent and learning vehicle
- collaboration style: explain Rust concepts when useful, avoid speculative
  abstractions, keep changes scoped
- exploration discipline: inspect files before editing, prefer `rg` for search,
  read local context before asking
- implementation discipline: preserve user changes, avoid destructive commands,
  use focused edits, verify before claiming completion
- answer style: concise final summaries, concrete file references, no hidden
  assumptions

This should stay cawir-specific. The goal is not to paste another agent's system
prompt into cawir; it is to encode the behavior we actually want this agent to
learn.

Rust topics: prompt section structs, section rendering tests, keeping stable
sections cache-friendly.

### Mode, permission, and workflow guidance

Prompt assembly should tell the model what mode it is currently in and how that
changes behavior.

Examples:

- `default`: read freely, ask before mutating tools
- `plan`: produce a plan, do not attempt mutating tools
- `accept-edits`: edits may proceed, shell still needs approval
- `bypass`: powerful mode, still avoid destructive actions unless explicitly
  requested

Today the tool set and policy code enforce those rules, but the system prompt does
not explain the active mode. Giving the model explicit mode context should reduce
denied tool attempts and make plan mode easier to reason about.

Rust topics: passing mode into prompt assembly, volatile vs cache-stable prompt
sections, tests for mode-specific prompt text.

### Tool prompt and schema quality

Review built-in tool descriptions as prompt-engineering artifacts, not just Rust
metadata.

Follow-ups:

- make `edit_file` the preferred routine edit path and reserve `write_file` for
  new files or full rewrites
- tell the model how to recover from visible truncation markers
- explain when to use `shell` instead of dedicated file tools
- document that external MCP/plugin tools may be slower, broader, or
  approval-gated
- add examples or tighter field descriptions where schemas are ambiguous

Tool definitions affect both model behavior and prompt-cache keys, so changes
should be deliberate and covered by stable fingerprint tests.

Rust topics: schema helper tests, golden tool-definition snapshots,
fingerprint-aware changes.

### Data-vs-instruction and prompt-injection boundaries

Add explicit prompt rules that tool outputs, file contents, command output, MCP
results, and plugin results are data to analyze, not higher-priority instructions
to obey.

This matters once the agent reads arbitrary repository files or external tool
results. A file can say "ignore previous instructions"; cawir should treat that as
file content unless the user explicitly asks to follow it.

Rust topics: prompt wording tests, provider-neutral section placement, regression
fixtures with hostile file content.

### Prompt inspection and debug tooling

Add a way to inspect what cawir is about to send before or during a turn.

Possible surfaces:

- `/prompt` in the REPL to render the current assembled system prompt
- `/tools` to show advertised tool definitions and fingerprint
- `cawir exec --show-prompt` or a protocol method for App Server clients
- redaction rules if future prompt sections include secrets or credential source
  metadata

This project is for learning, so seeing the prompt is a feature, not just a debug
escape hatch.

Rust topics: command registry additions, rendering without making a model call,
redaction helpers, protocol result shapes.

### Prompt regression and behavior tests

Add tests that lock down prompt assembly and expected agent behavior around common
coding tasks.

Start with offline tests:

- snapshot rendered prompt sections for a fixture project
- snapshot tool definitions and descriptions
- verify nested instruction ordering and duplicates
- verify active skill insertion and non-activated skill omission
- fake-provider scenarios where the model should choose `read_file`,
  `edit_file`, `shell`, or `exit_plan_mode`

Live model evals can come later. The first goal is to make prompt changes
reviewable instead of invisible.

Rust topics: fixture workspaces, golden snapshots without snapshot-test crates at
first, fake provider behavior tests.

## Context management and compaction backlog

Current state: cawir has a solid prompt, tool, and cache foundation, but it does
not yet have a full context-window management layer. Built-in broad tools are
bounded, provider usage and cache counts are observable after requests, and
Anthropic prompt caching is wired. However, the agent still sends the full durable
session history on every provider request, and there is no preflight check that
compacts before a new input would exceed the model's context window.

These follow-ups should become a deliberate context-management checkpoint before
cawir relies on long-running sessions, large MCP/plugin tools, or richer prompt
context.

### Recommended order

1. Provider/model context-window metadata.
2. Approximate request token estimation.
3. Preflight context budget before each provider call.
4. Separate budgets for prompt sections, tool definitions, active skills, and
   history.
5. Session compaction strategy and durable summary representation.
6. External tool output budgets for MCP and plugin tools.
7. Tests that prove compaction happens before a provider request would be too
   large.

### Provider and model context metadata

Add a provider-neutral way to know the active model's approximate context window
and reserve space for output tokens and safety margin.

Start conservatively: known defaults per built-in model are enough for the first
version. Dynamic model metadata can come later if provider APIs expose it
reliably.

Rust topics: provider metadata methods, model capability tables, conservative
fallbacks, keeping provider-specific facts out of the agent loop.

### Approximate request token estimation

Add a request estimator that can measure or approximate the size of the next
model request before sending it.

The first version does not need exact provider tokenization. A documented
character-to-token approximation is enough to prevent the worst failure mode:
"the next user input pushes the already-large session past the context limit and
the provider rejects the request."

Rust topics: owned request summaries, byte vs character counts, conservative
estimation, tests around threshold behavior.

### Preflight context budget

Introduce a context-management step before `ProviderRequest` construction reaches
the provider.

The step should inspect prompt sections, active tools, active skills, and session
history, then either allow the request, compact first, or return a clear local
error if compaction cannot make the request fit.

Rust topics: pure planning functions over `Session`, explicit budget decisions,
error types for local preflight failures.

### Budget request layers separately

Treat request inputs as separate budget layers instead of one blob:

- system prompt sections: identity, behavior, environment, project guidance
- active skill instructions
- provider-facing tool definitions
- durable conversation history
- newest user prompt and fresh tool-result tail

This keeps cache behavior explainable and gives future compaction logic clear
targets. For example, older tool results are good compaction candidates, while the
newest user prompt and recent tool calls should usually stay verbatim.

Rust topics: small structs for budget reports, stable ordering, readable debug
output.

### Session compaction strategy

Add a first `CompactionStrategy` that summarizes older history and keeps recent
turns verbatim.

The summary should be durable session data, not an ephemeral prompt-only string,
so `/resume` preserves the same compacted context. The internal session format
may need a new message or context-summary shape that provider adapters translate
into their native request formats.

Rust topics: enum evolution with serde compatibility, session schema versioning,
summaries as data instead of display text.

### External tool output budgets

Built-in `read_file`, `list_files`, and `shell` outputs are capped, but MCP and
plugin tool outputs can still inject unbounded content into history.

Add a shared output-budget helper for external tools so MCP and plugin results
get visible truncation markers before they become `tool_result` blocks. Keep the
marker explicit so the model knows it saw only part of the output.

Rust topics: shared helper extraction, UTF-8-safe truncation, applying one policy
across built-in and external tool implementations.

### Regression tests

Add tests that prove the context manager acts before a provider request is sent:

- large existing history plus a new prompt triggers compaction
- a compacted session still preserves recent turns verbatim
- external tool output truncation is visible in the resulting `tool_result`
- if compaction still cannot fit, the provider is not called and the user gets a
  local error

Rust topics: fake provider objects, deterministic session fixtures, testing
negative paths without network calls.

## App Server and surface backlog

Checkpoint 14 proved the foundation: App Server is the reusable boundary, `exec`
and TUI can drive the same agent loop, and the protocol can travel over stdio JSONL
or WebSocket. Most of that work was intentionally MVP-shaped. These are the main
follow-ups left by that checkpoint.

### Recommended order

1. Async approval boundary.
2. Multi-session App Server state.
3. Multi-client WebSocket daemon semantics.
4. TUI polish once the server boundary is stronger.
5. Exec automation and protocol/client cleanup.
6. Remote-readiness guardrails before any non-local deployment.

### Async approval boundary

Replace the synchronous approval callback bridge with an async approval path that
can naturally wait for protocol client responses without blocking a Tokio worker
thread.

Keep REPL approval working, but adapt it into the new boundary instead of letting
App Server carry a special blocking path forever.

Rust topics: futures returned from callbacks, boxed futures or async trait tradeoffs,
lifetimes across `.await`.

### Multi-session App Server state

Let one App Server process manage more than one session by id instead of storing a
single active `Runtime`/`Session`.

`turn/submit` should address an explicit session, and session lifecycle should be
clear enough for future clients to create, resume, list, and close sessions.

Rust topics: maps of owned runtime state, borrowing one session mutably while
routing protocol messages, avoiding long-lived mutable borrows across `.await`.

### Multi-client WebSocket daemon semantics

Accept more than one WebSocket client and define ownership rules for turns, events,
approvals, disconnects, and shutdown.

This is where "close this connection" separates from "stop the daemon." Keep the
first version local-only unless there is a concrete remote-use reason.

Rust topics: `tokio::spawn`, connection tasks, channels, shared state with `Arc`
and async-aware locking.

### TUI client polish

Improve the MVP TUI as an App Server client:

- scroll transcript and tool panes
- render richer approval details such as command, file, or diff context
- add session/provider/model/mode controls
- handle narrow terminals better
- optionally connect to an existing WebSocket App Server instead of always spawning
  a stdio child

Rust topics: richer Ratatui state machines, viewport math, input modes, client
transport selection.

### Exec automation hardening

Make `cawir exec` more script-friendly:

- clearer exit-status mapping for protocol, model, and tool failures
- timeout and cancellation controls
- documented JSON output
- possibly an option to connect to an existing App Server instead of always
  spawning a stdio child

Rust topics: process exit codes, timeout futures, cancellation and cleanup.

### Protocol and client SDK cleanup

Move shared protocol message types out of `app_server.rs` / `app_client.rs`
duplication into a clearer protocol module.

Document example message flows and consider a small reusable client abstraction for
stdio and WebSocket. Keep JSON-RPC message semantics stable while making the code
easier for future clients to reuse.

Rust topics: module boundaries, public vs `pub(crate)` API choices, serde
compatibility.

### Remote-readiness guardrails

Before any non-local WebSocket use, define the minimum safety story:

- bind defaults
- auth or token strategy
- TLS expectations
- origin/CORS considerations for browser clients
- logging that does not leak secrets

This can stay documentation-first until there is a real remote client.

Rust topics: configuration parsing, secret handling, conservative defaults.
