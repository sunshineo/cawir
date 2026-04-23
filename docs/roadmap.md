# cawir roadmap

## Philosophy

Two equal goals: **build a working coding agent** AND **learn Rust**. Every checkpoint moves both forward. If a checkpoint feels too big for a single work session, split it. If it teaches zero new Rust, it probably shouldn't be a distinct checkpoint.

- **Each checkpoint is usable.** You can run cawir at every checkpoint and it does something concrete you couldn't do before.
- **Rust concepts are introduced, not dumped.** One to three new concepts per checkpoint. When we hit a fourth, we pause and split.
- **Architecture is a reference, not a blueprint.** No speculative abstractions. When the Rule of Three kicks in (two concrete impls before extracting a trait), we extract — not before.
- **This roadmap is alive.** Checkpoints will split, merge, reorder. Things will change.

## Three phases, nine checkpoints

| Phase | Checkpoints |
|---|---|
| **Foundation** (not yet an agent) | 1. Echo · 2. Chat |
| **The agent** | 3. Agent loop ⭐ · 4. Modes |
| **Craft** | 5. Streaming · 6. Multi-model · 7. Hooks · 8. Polyglot · 9. Resume |

---

## Checkpoint 1 — Echo

**Goal.** A minimal REPL that reads user input and prints it back. Recognizes `/exit` and `/help` as local slash commands.

```
$ cargo run
cawir> hello
you said: hello
cawir> /help
  /exit   quit the REPL
  /help   show this help
cawir> /exit
```

**Rust concepts introduced.**

- `String` vs `&str` — when do you own text vs borrow it?
- `std::io::{stdin, BufRead, Write}` — line-based I/O
- `Result<T, E>` and the `?` operator
- `loop` and `break`
- `match` on string slices

**Architecture components touched.** Surface.

**Done when.** You can type something, see it echoed, type `/exit`, and the program exits cleanly. `/help` shows the two available commands.

---

## Checkpoint 2 — Chat

**Goal.** cawir sends your prompt to Claude and prints the response. Multi-turn — Claude remembers earlier turns in the same session.

```
cawir> hello, what's your name?
claude: I'm Claude, made by Anthropic.
cawir> what did I just ask?
claude: You asked what my name is.
```

**Rust concepts introduced.**

- Adding crates: `reqwest`, `tokio`, `serde`, `serde_json` (walk through why each is the right choice)
- `async`/`await` — the `#[tokio::main]` entry point, awaiting futures
- `#[derive(Serialize, Deserialize)]` — making Rust types talk to JSON
- `Vec<T>` for conversation history
- Structs and enums with variant data — the `Session`, `Message`, `ContentBlock` types from the architecture doc
- Modules — the first `main.rs` → `main.rs` + `lib.rs` + `session.rs` + `error.rs` split (architecture decisions #6, #7)
- `thiserror` — proper error enum from v0.1
- `std::env::var` — reading `ANTHROPIC_API_KEY`

**Architecture components touched.** Surface, Core engine (minimal agent loop — no tool dispatch yet), External (a hard-coded Anthropic call, not yet a `Provider` trait).

**Done when.** You can have a 5-turn conversation with Claude about anything, and Claude's replies show awareness of earlier turns.

**Watch-out.** The biggest single-checkpoint Rust concept jump — async + crates + serde + modules all landing at once. If it feels overwhelming mid-flight, split: "first call" (one prompt, one response) before "conversation" (multi-turn with `Vec<Message>`).

---

## Checkpoint 3 — Agent loop ⭐

**The soul of cawir.** Everything before this has been setup — a Rust REPL, an HTTP client, a chat interface. None of those are agents. This checkpoint is where cawir becomes what its name says: a Coding Agent.

**The agent loop is the pattern:** the model calls a tool, we execute it, we feed the result back, the model keeps thinking — and it can call more tools, receive more results, and continue until it's satisfied. You type *"what are the dependencies in Cargo.toml?"* and cawir autonomously reads the file and answers. You type *"add pretty_assertions as a dev dependency,"* and cawir reads Cargo.toml, edits it, writes the result, tells you done.

That loop is the entire magic behind every coding agent — Claude Code, Cursor, Aider, all of them. After this checkpoint, you have built it.

**Goal.** cawir has a `Tool` trait, a registry with several tools (`read_file`, `write_file`, `shell`), and an agent loop that dispatches tool calls, executes them, and feeds results back to the model — repeating until the model stops calling tools.

```
cawir> what are the dependencies in Cargo.toml?
[claude calls read_file(path="Cargo.toml")]
[reading Cargo.toml... done]
claude: Your dependencies are: reqwest, tokio, serde, serde_json.

cawir> add pretty_assertions as a dev dependency
[claude calls read_file(path="Cargo.toml")]
[claude calls write_file(path="Cargo.toml", content="...")]
approve write to Cargo.toml? (y/n) y
[writing... done]
claude: Added pretty_assertions = "1.4" to [dev-dependencies].
```

**Rust concepts introduced.**

- **Traits** — our first `trait Tool`. The foundational Rust abstraction.
- `#[async_trait]` — the crate that lets traits have async methods
- Trait objects — `Arc<dyn Tool>`, storing heterogeneous tools in one registry
- `HashMap<String, Arc<dyn Tool>>` — the registry structure
- `serde_json::Value` — dynamic JSON shapes for tool input schemas
- `std::fs::read_to_string`, `std::fs::write`
- `std::process::Command` — for the shell tool
- A simple inline permission prompt (stdin mid-loop) — will be replaced by proper modes in Checkpoint 4

**Architecture components touched.** Capabilities (Tool trait + tool impls + ToolRegistry), Core engine (agent loop grows a real tool-use branch), Surface (prompt-for-approval UX inline for now).

**Done when.** You can ask cawir to perform a multi-step task that requires reading files and writing files — and it actually does it. At minimum: *"read Cargo.toml and tell me the dependencies"* works end-to-end without you mediating.

**Watch-out.** Traits are a big concept. If introducing the Tool trait plus three tools plus the loop plus approval UX in one session is too much, split within the checkpoint: get the loop working with just `read_file`, prove it, then add `write_file` and `shell` without touching the loop. Same checkpoint, same artifact at the end.

---

## Checkpoint 4 — Modes

**Goal.** Formalize the permission system. Replace the inline "ask before every write" with a `PermissionMode` enum (`Default`, `Plan`, `AcceptEdits`, `Bypass`), a `/mode <name>` slash command, and — most fun — plan mode, where Claude researches read-only and produces a plan you approve before it executes.

```
cawir> /mode plan
(plan mode active — writes and shell disabled)

cawir> refactor main.rs into smaller modules
[claude reads files, thinks...]
[claude calls exit_plan_mode(plan="...")]

Plan:
  1. Move Session, Message types into session.rs
  2. Extract HTTP client setup into http.rs
  3. Update main.rs imports

approve plan? (y/n/keep planning) y
(plan mode exited — executing)
[claude calls write_file...]
```

**Rust concepts introduced.**

- `enum` as a state machine (`PermissionMode`)
- Exhaustive `match` on enum variants — and why the compiler refuses to compile if you miss one
- Special tools that don't mutate — `ExitPlanMode` emits an event-like signal instead of executing a side effect. First taste of the lifecycle-event pattern before the full event bus lands in Checkpoint 7.

**Architecture components touched.** Policy (the full Permission module arrives).

**Done when.** `/mode plan` restricts writes; `exit_plan_mode` renders a plan and prompts for approval; `/mode default` restores normal behavior; `/mode bypass` allows everything without prompts.

---

## Checkpoint 5 — Streaming

**Goal.** Claude's output appears token-by-token as it's generated, rather than arriving as a finished block after a pause.

```
cawir> write me a haiku about Rust
claude: Ownership compiles
        through lifetimes and borrows
        safe and fearless code
```

(where each line visibly types out as you watch)

**Rust concepts introduced.**

- `futures::Stream` and `StreamExt` — the async iterator pattern
- `async fn` returning `impl Stream<Item = T>`
- SSE (Server-Sent Events) parsing — Anthropic's streaming format
- Partial-JSON handling — deltas that form complete messages over time

**Architecture components touched.** External (the Anthropic call becomes stream-returning), Core engine (agent loop consumes a stream instead of awaiting a single response; tool calls still block the stream until their result comes back, then streaming resumes from the same response cycle).

**Done when.** Responses visibly stream. Tool calls still work — the loop correctly detects when a `tool_use` block is complete, executes it, and resumes streaming from the same response cycle.

---

## Checkpoint 6 — Multi-model

**Goal.** Add OpenAI as a second provider. **Now** extract the `Provider` trait from the two concrete implementations. Rule of Three (here, Rule of Two) — the abstraction emerges from the diff between two real impls, not from planning ahead.

```
cawir> /provider openai
(switched to openai)
cawir> hello
gpt: Hi!

cawir> /provider anthropic
(switched to anthropic)
cawir> what did I just say?
claude: You said "hello" — but that was to GPT, not me.
```

**Rust concepts introduced.**

- **Extracting a trait from two concrete impls** — the canonical Rust abstraction moment
- Static dispatch (generics `<P: Provider>`) vs dynamic dispatch (`Box<dyn Provider>`) — when to use which
- Trait methods with default implementations
- Optional pause for a community article on "Rule of Three" — this is a design skill worth reflecting on

**Architecture components touched.** External (the Anthropic code from Checkpoint 2 moves into `src/provider/anthropic.rs`; OpenAI gets `src/provider/openai.rs`; the trait lives in `src/provider/mod.rs`).

**Done when.** `/provider openai` and `/provider anthropic` both produce working conversations. The same `Session` message history round-trips between them when supported.

---

## Checkpoint 7 — Hooks

**Goal.** Wire up the event bus. Agent loop emits `AgentEvent` values at lifecycle points; a `HookRegistry` dispatches to handlers loaded from `settings.json`. One working hook: run `cargo fmt` after any `write_file` to a `.rs` file.

Example settings config:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": { "tool": "write_file", "path": "*.rs" },
        "command": "cargo fmt"
      }
    ]
  }
}
```

Running:

```
cawir> fix the indentation in src/main.rs
[claude calls read_file...]
[claude calls write_file...]
[hook: cargo fmt]
claude: Done — I reformatted the indentation, and your post-hook ran cargo fmt.
```

**Rust concepts introduced.**

- `Arc<Mutex<T>>` or `tokio::sync::RwLock` — shared state in async code
- `tokio::sync::broadcast` or channels — async event distribution
- `Arc<dyn Trait>` arrays as subscribers — the hook registry's dispatch table
- File I/O for loading settings.json
- JSON merge semantics for the settings hierarchy (user → project → local)

**Architecture components touched.** Core engine (events get emitted; hook dispatch runs synchronously before events flow to the REPL stream), Capabilities (HookRegistry + HookHandler trait + command-handler impl), External (SettingsResolver).

**Done when.** A hook configured in settings.json actually runs on the configured event. `PreToolUse` denial works: a hook can return `HookAction::Deny` and prevent a tool from running.

---

## Checkpoint 8 — Polyglot

**Goal.** Add Ollama as a third provider. Introduce `AuthMethod` as an orthogonal trait. Build the credential lookup chain: macOS Keychain → env var → `.env` file.

```
cawir> /provider ollama
(switched to ollama, model qwen2.5-coder:7b)
cawir> hello
ollama: Hi there!
```

**Rust concepts introduced.**

- A second orthogonal trait (`AuthMethod`) composing with `Provider`
- `keyring` crate — macOS Keychain access
- `dotenvy` crate — `.env` file loading
- The "one component, multiple sources" pattern generalizes — `ToolRegistry` and `HookRegistry` already do it; `CredentialChain` is the newest application

**Architecture components touched.** External (AuthMethod trait + impls, CredentialChain).

**Done when.** `/provider ollama` works with no credentials. `/provider openai` works with either `OPENAI_API_KEY` (env) or an API key stored in Keychain. Switching providers mid-session works.

---

## Checkpoint 9 — Resume

**Goal.** `cawir --resume <session-id>` picks up a previous conversation where you left off. `cawir --continue` resumes the most recent session.

```
$ cawir
cawir> what's the capital of France?
claude: Paris.
cawir> /exit
(session 7f3e8a saved)

$ cawir --resume 7f3e8a
(resumed session 7f3e8a)
cawir> what did I just ask?
claude: You asked what the capital of France is.
```

**Rust concepts introduced.**

- Serde in action — writing `Session` to disk, reading it back. The type shape is already right (architecture decision #9 paid off).
- CLI argument parsing with `clap` — `--resume <id>`, `--continue`
- Path handling with the `directories` crate — macOS `~/Library/Application Support/cawir/sessions/`
- File system basics — creating directories, listing files, JSON roundtrips

**Architecture components touched.** Core engine (Session now persists), Surface (new CLI flags and `/resume` slash command).

**Done when.** A conversation survives an `/exit` + restart.

---

## Rust concepts by checkpoint

| # | Checkpoint | New Rust concepts |
|---|---|---|
| 1 | Echo | `String`/`&str`, `stdin`, `Result`, `?`, `loop`, `match` |
| 2 | Chat | crates, `async`/`await`, serde derive, `Vec`, structs/enums, modules, `thiserror`, env vars |
| 3 | Agent loop | **traits**, `#[async_trait]`, trait objects, `HashMap`, `serde_json::Value`, `std::fs`, `Command` |
| 4 | Modes | `enum` as state machine, exhaustive match, special tools |
| 5 | Streaming | `futures::Stream`, `StreamExt`, SSE parsing, partial JSON |
| 6 | Multi-model | **extracting a trait from two impls**, static vs dynamic dispatch |
| 7 | Hooks | `Arc`/`Mutex`, channels, async dispatch, JSON merge |
| 8 | Polyglot | second orthogonal trait (composition), `keyring`, `dotenvy` |
| 9 | Resume | serde for disk I/O, `clap`, `directories` |

## Beyond Checkpoint 9 (speculative — not committed)

Features not on the roadmap but with architectural seams already identified:

- **MCP tool support** — plugs into `ToolRegistry`
- **Plugin loading** — plugs into `ToolRegistry`, `HookRegistry`, slash-command discovery
- **Subagents** — a `SubAgent` tool that instantiates another `agent::run(...)` loop on a nested `Session`
- **Auto-mode permission classifier** — `PermissionMode::Auto` backed by an LLM call
- **Context compaction** — `CompactionStrategy` trait applied to `Session`
- **Session memory extraction** — consolidates `Session` into durable memory at `SessionEnd`
- **TUI upgrade** — swap the REPL for a `ratatui` consumer of the same event stream
- **Remote transport** — WebSocket or SSE consumer of the same event stream

Each has an architectural home already. When we reach Checkpoint 9, we'll decide which (if any) to tackle next based on what feels alive.
