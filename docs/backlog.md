# cawir backlog

This file holds concrete follow-ups that are worth keeping but are not part of the
main checkpoint sequence yet. Promote an item into [`roadmap.md`](roadmap.md) only
when we decide to actively work it as a checkpoint or sub-step.

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
