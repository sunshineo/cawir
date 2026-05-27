# Agent events and surfaces

Checkpoint 8 introduced a typed event stream between the agent loop and the terminal REPL.

Before this checkpoint, the core loop did two jobs:

```text
orchestrate the model/tool loop
print progress to the terminal
```

Now the core loop emits structured `AgentEvent` values, and the REPL decides how to render them.

## Events are progress, not conversation history

`AgentEvent` is for observers:

```rust
AgentEvent::PreToolUse { id, name, input }
AgentEvent::PostToolUse { id, name, output_len, is_error, error }
AgentEvent::AssistantText { provider, text }
```

These events are useful for terminal rendering, future hooks, logs, JSON output, and a future TUI.

`tool_result` is different. It is model-facing session data:

```rust
MessageContent::ToolResult {
    tool_use_id,
    content,
    is_error,
}
```

The provider needs `tool_result` blocks so the model can continue after a tool call. The session needs them so resumed conversations preserve the model context.

The REPL usually does not need to print the full tool result. It can render a progress summary:

```text
tool result from read_file: 1234 bytes
```

So cawir now keeps two streams separate:

- `AgentEvent`: transient progress and display data.
- `MessageContent::ToolResult`: durable conversation and provider protocol data.

## Why events are an enum

The current event vocabulary is small and owned by cawir:

```rust
pub(crate) enum AgentEvent {
    SessionStart { session_id: String, ... },
    UserPromptSubmit { prompt: String },
    ModelRequestStart { provider: String, model: String },
    ModelRequestFinish { provider: String, model: String, metadata: ProviderMetadata },
    PreToolUse { id: String, name: String, input: Value },
    PostToolUse { ... },
    AssistantText { ... },
    Stop { reason: StopReason },
    StopFailure { kind: FailureKind, message: String, retryable: bool },
    SessionEnd { session_id: String },
}
```

An enum says:

```text
an event is exactly one of these known shapes
```

That fits better than a trait object while the event vocabulary is closed. A trait object is useful when external code must define new event types. cawir is not there yet.

The enum also keeps events as plain data, which is useful for tests and later JSON serialization.

## Producer and consumer boundary

The agent loop accepts an event consumer:

```rust
emit: &mut impl FnMut(AgentEvent)
```

The agent is the producer:

```rust
emit(AgentEvent::ModelRequestStart { ... });
```

The REPL is one consumer:

```rust
fn render_agent_event(event: AgentEvent) {
    match event {
        AgentEvent::PreToolUse { id, name, .. } => {
            println!("tool request: {name} ({id})");
        }
        AgentEvent::AssistantText { provider, text } => {
            println!("{provider}: {text}");
        }
        _ => {}
    }
}
```

Tests can be another consumer by collecting events into a vector.

This is not a thread boundary. It is normal function calling. The callback runs immediately when `agent.rs` emits the event.

## Pre-tool means the raw requested boundary

Checkpoint 8.5g renamed the tool events from `ToolUseRequested` / `ToolUseFinished` to `PreToolUse` / `PostToolUse`.

`PreToolUse` currently fires before `registry.execute(...)`. That means it describes the raw model-requested tool call before cawir has prepared input, applied permission policy, asked for approval, or executed the tool.

This is the earliest useful hook point. A future hook may want to reject a tool call before any local behavior happens. Later, if hooks need the canonical prepared form too, cawir can add a second event point after preparation and policy validation. That pressure does not exist yet.

`PostToolUse` summarizes what happened after execution: original input, output length, error flag, and optional error string. The full tool output still belongs in `MessageContent::ToolResult`, not in the event.

Checkpoint 8.5i added the original input to `PostToolUse` for hooks. A command hook receives one event JSON object on stdin; it should not have to remember a previous `PreToolUse` event just to answer basic questions like:

```text
which file did write_file touch?
was the target path a Rust file?
should a post-write formatter run?
```

This intentionally duplicates a small amount of data between pre-tool and post-tool events. The alternative would force every stateless command hook to build its own correlation store keyed by tool id, which is too much machinery before hooks even exist.

## Why the REPL renders

`agent.rs` owns orchestration:

```text
send provider request
handle provider response
execute tools
append messages
decide when the turn stops
```

`repl.rs` owns the terminal surface:

```text
prompt for input
print progress
ask approval questions
render transcripts
handle slash commands
```

Keeping rendering out of `agent.rs` makes the core loop easier to reuse. A future TUI, JSON command surface, or hook runner can consume the same events without changing model/tool orchestration.

## Serialization is now an event contract

Checkpoint 8 kept `AgentEvent` as internal Rust data. Checkpoint 8.5g moved it closer to an external contract:

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AgentEvent {
    // ...
}
```

This serializes each event with a stable `type` field:

```json
{ "type": "pre_tool_use", "id": "toolu_123", "name": "read_file", "input": { "path": "src/main.rs" } }
```

That shape is easy for hooks, logs, JSON output, a TUI, or a WebSocket surface to consume. The cost is that event names and fields become harder to rename casually. Once external consumers match on `pre_tool_use`, changing that string is an API break.

## App Server before UI polish

OpenAI's Codex App Server write-up sharpened the CP14 direction: the foundation is
not "add a TUI" or "add WebSocket" first. The foundation is a protocol boundary that
lets rich clients drive the same harness without linking to surface internals or
reimplementing the loop:
<https://openai.com/index/unlocking-the-codex-harness/>

For cawir, that means:

- `cawir app-server` starts as stdio JSONL with JSON-RPC-style request, response,
  and notification envelopes.
- WebSocket is a later transport for the same protocol, not the protocol itself.
- `exec`, TUI, and future IDE-style surfaces should reuse the same turn/session path
  behind the app server.
- Approval prompts are bidirectional protocol interactions: the server can ask the
  client for a decision and pause the turn until the client responds.

This also raises the cost of event/protocol churn. Once clients consume an
app-server event, names and payloads become compatibility promises.

## JSON-RPC messages are shape-based

JSON-RPC-style protocols usually do not wrap every message in a top-level
`type` field. Instead, the message kind is inferred from fields:

```json
{"id":1,"method":"initialize","params":{}}
```

This is a client request: the `id` means the client expects a response.

```json
{"method":"initialized","params":{}}
```

This is a client notification: no `id`, so the server should not answer.

```json
{"id":1,"result":{}}
```

This is a success response.

```json
{"id":1,"error":{"code":-32601,"message":"method not found"}}
```

This is an error response.

That shape-based style is why 14a parses app-server input differently from
`AgentEvent`. `AgentEvent` is cawir-owned, so a stable explicit `type` field is
clearer. App Server follows a JSON-RPC-style boundary, so requests and notifications
are recognized by their fields.

Bad client input should be part of the protocol, not a process crash:

- malformed JSON becomes a parse-error response
- valid JSON with the wrong message shape becomes an invalid-request response
- unknown methods become method-not-found responses

This distinction matters for rich clients. An IDE, TUI, or future WebSocket client
needs structured failures it can display or recover from without guessing from
stderr text.

## Shared runtime below surfaces

Checkpoint 14b moved the reusable non-UI runtime pieces out of `repl.rs` and into
`runtime.rs`.

The split is:

```text
runtime.rs: provider handle, credential, model, HTTP client, registries, session sync,
            project context loading, generic turn execution

repl.rs:    terminal input/output, slash commands, transcript rendering, interactive
            credential prompts, interactive tool/plan approvals
```

This keeps the REPL as one surface instead of the owner of the harness. A future
App Server method can create/resume sessions and run turns by using the same runtime
path, then supply protocol callbacks for approvals and event notifications instead
of terminal prompts.

This is a different boundary from `agent.rs`. The agent loop still owns the model
and tool orchestration for one turn. `runtime.rs` owns the reusable application
handles and repeated "run until complete, including plan approval continuations"
policy that every surface needs.

## App Server as a stateful protocol surface

The first useful App Server boundary is stateful. `initialize` only negotiates the
protocol. After that, a client creates or resumes one active session:

```text
session/new    -> prepare Runtime + Session
session/resume -> load Session + prepare matching Runtime
turn/submit    -> append user prompt, run the shared runtime turn loop
```

During `turn/submit`, the server sends `AgentEvent` values as notifications:

```json
{"method":"event","params":{"session_id":"...","event":{"type":"assistant_text_delta","text":"..."}}}
```

Approval is bidirectional. When the model asks for a mutating tool or plan exit,
the server sends a request to the client and waits for a response:

```json
{"id":"server-1","method":"approval/tool","params":{"tool_name":"write_file","summary":"..."}}
{"id":"server-1","result":{"approved":true}}
```

That keeps policy decisions in the same turn loop while letting each surface decide
how approval is rendered. The REPL asks with terminal text; App Server asks with a
protocol request.

The App Server must not print credential setup prompts to stdout, because stdout is
the JSONL protocol stream. For now it only uses already-configured credentials and
returns structured errors when credentials are missing. Interactive credential setup
stays in REPL until there is a protocol-shaped credential flow.

## Non-interactive means protocol-shaped interaction

Calling the App Server "non-interactive" does not mean clients can never ask a
human for a decision. It means the server itself does not print ad hoc terminal
prompts and then read arbitrary human text from stdin.

The REPL owns a human terminal, so it can do this:

```text
approve write_file? [y/N]
```

The App Server owns a protocol stream:

```text
stdin  = JSONL requests and responses from the client
stdout = JSONL responses, notifications, and server requests
```

So every interaction must be shaped as protocol data:

```json
{"id":"server-1","method":"approval/tool","params":{"tool_name":"write_file"}}
{"id":"server-1","result":{"approved":true}}
```

That is still interactive at the product level. A TUI, IDE extension, or web
client may show a dialog to the user. The important boundary is that App Server
only sees structured client responses, not terminal keystrokes.

## Exec is the first App Server client

Checkpoint 14c makes `cawir exec "..."` a client of `cawir app-server` instead
of another direct caller of `runtime.rs`.

The process shape is:

```text
cawir exec "prompt"
  starts child process: cawir app-server
  sends initialize
  sends session/new or session/resume
  sends turn/submit
  reads event notifications and approval requests
  sends shutdown
```

This is the first time the App Server is visible as a real foundation instead of
only a manually testable JSONL loop. The one-shot CLI gets the same protocol path
that a future TUI or IDE client would use.

The boundary is intentionally small. `exec` does not own provider setup,
session loading, tool execution, hooks, skills, or the model loop. It only owns
client behavior:

- render assistant text for a human CLI
- render structured JSONL when `--json` is requested
- answer App Server approval requests with a simple policy
- turn protocol errors into CLI errors

Because `exec` is headless, approval must be deterministic. It should not pause
for terminal input when App Server sends `approval/tool` or `approval/plan`.
The 14c policy is:

```text
default   -> answer approved=false
--approve -> answer approved=true
```

That keeps scripts from hanging on an unseen prompt. If a future surface wants
human-in-the-loop approvals, that belongs in a TUI, IDE client, or another
explicitly interactive command, not in the headless `exec` default path.

This keeps the direction honest: new surfaces should become protocol clients or
thin adapters, not copies of the harness.

## TUI proves value through persistent state

Checkpoint 14d starts the TUI as another App Server client. Its first job is not
to be a full terminal IDE. Its job is to show why a terminal UI is more powerful
than the REPL.

The REPL is a single linear stream:

```text
prompt -> output -> prompt -> output
```

The TUI keeps several pieces of state visible at the same time:

- transcript pane for user and assistant messages
- status pane for provider, model, mode, session, and keybindings
- tool timeline pane for requested and completed tools
- approval pane for pending `approval/tool` or `approval/plan`
- input pane for the next user prompt

That makes the same App Server event stream easier to understand. Events no
longer have to be interleaved into one scrollback log; each event can update the
part of the interface it belongs to.

The important design rule is the same as `exec`: the TUI is a client. It starts
or connects to App Server, sends JSONL requests, receives event notifications,
answers server approval requests, and renders the result. It should not copy the
provider/session/tool loop.

## Structured failures serve machines and humans

Checkpoint 8 had `StopFailure { message: String }`. That was readable, but future hooks or alternate surfaces would have had to parse a human string to answer basic questions.

Checkpoint 8.5g changed failures to:

```rust
StopFailure {
    kind: FailureKind,
    message: String,
    retryable: bool,
}
```

`message` stays because people need context. `kind` and `retryable` exist so code can branch without scraping prose. For example, a future surface can render provider failures differently from tool-loop-limit failures, and a future retry policy can look at `retryable`.
