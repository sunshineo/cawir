# cawir backlog

This file holds concrete follow-ups that are worth keeping but are not part of the
main checkpoint sequence yet. Promote an item into [`roadmap.md`](roadmap.md) only
when we decide to actively work it as a checkpoint or sub-step.

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
