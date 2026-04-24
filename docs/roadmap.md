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

**Sub-steps** — split to keep each step's Rust surface minimal:

### 1a — First read

Print a prompt, read one line from stdin, print it back, exit.

- *New Rust:* `std::io::{stdin, Write}`, `String` (owned text), `Result<T, E>`, the `?` operator, `print!` + `flush` (why `println!` isn't enough for a prompt).
- *Done when:* `cargo run` prompts `cawir> `, accepts one line of input, echoes it back, and exits.

### 1b — Loop forever

Wrap 1a in a `loop`. Exit on Ctrl-D (EOF) or when `read_line` returns `Ok(0)`.

- *New Rust:* `loop` and `break`, pattern matching on `read_line`'s return value to detect EOF.
- *Done when:* you can type many lines and each is echoed. Ctrl-D ends the program cleanly.

### 1c — Slash commands

Recognize `/exit` (quit) and `/help` (print the command list). Anything else echoes.

- *New Rust:* `match` on `&str`, `str::trim`, `str::starts_with`, the difference between `String` (owned) and `&str` (borrowed) when comparing.
- *Done when:* `/exit` quits cleanly; `/help` prints a two-line command list; anything else echoes.

#### Heads up — the CommandRegistry comes later

With only `/exit` and `/help`, a hardcoded `match` on strings is the right solution. Rule of Three isn't met; speculation would pick a wrong abstraction. The `Command` trait + `CommandRegistry` (multi-source: built-in, settings-loaded, plugin-loaded) emerges later — most likely around Checkpoint 6 (when `/provider <name>` introduces the first command with an argument) or Checkpoint 7 (when hook-configured commands add the first dynamic source). Plugin-loaded slash commands are further out and currently speculative.

Write the 1c match in a shape that's one refactor from a registry lookup:

```rust
match trimmed {
    "/exit" => break,
    "/help" => print_help(),
    other   => println!("you said: {}", other),
}
```

Each arm corresponds to what will later be a registry entry. Extracting `CommandRegistry` = moving the arms into a `HashMap<&str, Box<dyn Command>>` and adding an argument type. No restructuring needed.

**Architecture components touched.** Surface.

**Final state.** Everything the goal promises — type something, see it echoed, `/exit` quits, `/help` lists commands.

---

## Checkpoint 2 — Chat

**Goal.** cawir sends your prompt to Claude and prints the response. Multi-turn — Claude remembers earlier turns in the same session.

```
cawir> hello, what's your name?
claude: I'm Claude, made by Anthropic.
cawir> what did I just ask?
claude: You asked what my name is.
```

This is the biggest Rust jump in the roadmap (async, crates, serde, modules, thiserror, env vars — all landing in one checkpoint). Breaking it into six small sub-steps keeps each one to one or two new concepts.

### 2a — First HTTP call

Fetch a plain-text endpoint (e.g. `https://api.github.com/zen` — returns a one-line zen quote). No Claude, no JSON — just proving the HTTP plumbing works.

- *New Rust:* adding crates to `Cargo.toml` (`reqwest`, `tokio`), `async`/`await`, `#[tokio::main]` attribute.
- *Done when:* `cargo run` fetches and prints a line of GitHub Zen.

### 2b — Parse JSON

Fetch a JSON endpoint and parse the response into a Rust struct. Suggested target: `https://api.github.com/repos/rust-lang/rust` (a fixed public endpoint, extract a couple of fields like `description` and `stargazers_count`).

- *New Rust:* `#[derive(Deserialize)]` with `serde`, `reqwest::Response::json::<T>().await?`, struct fields with `#[serde(rename = "...")]` when needed.
- *Done when:* `cargo run` prints, e.g., the star count and description of the rust-lang/rust repo.

### 2c — First Claude call

Hard-coded "hello" POST to Anthropic's `/v1/messages`. Print Claude's reply.

- *New Rust:* `#[derive(Serialize)]` for request bodies, custom HTTP headers (`anthropic-version`, `x-api-key`, `content-type`), `std::env::var` for `ANTHROPIC_API_KEY`, basic error handling with `Box<dyn Error>` (we'll upgrade to `thiserror` in 2f).
- *Done when:* `cargo run` prints a Claude-generated reply to a hard-coded "hello" prompt.

### 2d — Wire with the REPL

Replace the hard-coded prompt with user input from the Checkpoint 1 REPL. Each line → one-shot Claude call → printed reply. No history yet — every message is independent.

- *New Rust:* combining a sync input loop with async calls (either restructure `main` as `async` under `#[tokio::main]`, or use a tokio runtime handle).
- *Done when:* you can ask Claude one question per line in the REPL and see replies.

### 2e — Multi-turn conversation

Maintain a `Vec<Message>` across turns. Send the full history on each call so Claude has context.

- *New Rust:* `Vec<T>` in practice, `struct` and `enum` with variant data (the `Session`, `Message`, `ContentBlock` shapes from `docs/architecture.md`). Pattern matching on enum variants.
- *Done when:* Claude's reply to turn N shows awareness of turn N-1.

### 2f — Cleanup

Split `main.rs` into `main.rs` + `lib.rs` + `session.rs` + `error.rs` per architecture decision #6. Replace `Box<dyn Error>` with a proper `thiserror` enum per decision #7. Establish the module pattern we'll use from here on.

- *New Rust:* `mod`, `pub`, `use`, module visibility rules, `#[derive(thiserror::Error)]`, `#[from]` attribute for error conversion.
- *Done when:* code is organized per the architecture doc's target module layout (even if only a subset of files exist); `main.rs` is thin; tests can sit next to modules.

**Architecture components touched.** Surface (reused from Checkpoint 1), Core engine (minimal — no tool dispatch yet), External (a hard-coded Anthropic call, not yet a `Provider` trait).

**Final state.** A working multi-turn conversation with Claude, all code organized in the module shape we'll grow from.

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

- **The agent-loop protocol itself** — "while there are tool calls, execute and send results back" — this is the conceptual heart of CP3, not a Rust concept per se but the thing you're actually learning here.
- `serde_json::Value` — dynamic JSON shapes for tool input
- `std::fs::read_to_string`, `std::fs::write`
- `std::process::Command` — for the shell tool
- `match` on tool names — dispatch pattern. You know `match` from 1c; this is its first async use.
- A simple inline approval prompt for mutating tools (`write_file`, `shell`) — hardcoded per-tool, not a formal system.

Notably NOT introduced at CP3: traits, trait objects, `HashMap`, async trait machinery. See the heads-up below.

**Architecture components touched.** Core engine (agent loop grows a real tool-use branch; tool dispatch is a `match` statement, not yet a registry), Surface (inline prompt-for-approval UX for mutating tools).

**Done when.** You can ask cawir to perform a multi-step task that requires reading files and writing files — and it actually does it. At minimum: *"read Cargo.toml and tell me the dependencies"* works end-to-end without you mediating.

**Watch-out.** If introducing three tools + the loop + approval UX in one session is too much, split within the checkpoint: get the loop working with just `read_file`, prove it, then add `write_file` and `shell` without touching the loop. Same checkpoint, same artifact at the end.

#### Heads up — Tool trait, ToolRegistry, and permission modes come later

Intentionally deferred from CP3:

| Deferred thing | Lands at | Why not now |
|---|---|---|
| **`Tool` trait + `ToolRegistry`** | Probably around CP6-CP7 (when the `Provider` trait gets extracted and a second reason — plugins/MCP/hook-registered tools — demands dynamic sourcing) | Three concrete tool functions is enough to *see* the shape but not yet enough to *need* a trait. Premature extraction picks the wrong abstraction. Wait for concrete pressure. |
| **`PermissionMode` enum + `/mode` command** | Checkpoint 4 | The inline `approve?` prompt on `write_file`/`shell` is safe enough for CP3. Formalizing modes is CP4's point. |
| **Plan mode + `ExitPlanMode` tool** | Checkpoint 4 | Depends on permission modes existing. |
| **Multi-source tool registration** (plugins, MCP, config-declared) | Post-CP9 speculative | Currently in the "Beyond CP9" list. |

Write CP3's tool dispatch in a shape that's one refactor from a trait-based registry:

```rust
let result = match tool_call.name.as_str() {
    "read_file" => tools::read_file(&tool_call.input).await?,
    "write_file" => {
        if !approve(&tool_call)? { return denied(); }
        tools::write_file(&tool_call.input).await?
    }
    "shell" => {
        if !approve(&tool_call)? { return denied(); }
        tools::shell(&tool_call.input).await?
    }
    other => return Err(Error::UnknownTool(other.into())),
};
```

Each tool is a plain `async fn(input: serde_json::Value) -> Result<ToolResult>`. When the `Tool` trait lands later, those function signatures become the trait method's signature verbatim — you're lifting them into `impl Tool for ReadFile { async fn execute(...) }`, with the match replaced by `registry.get(name).execute(input)`. The approval check becomes a `PermissionMode::Default` rule in CP4. No restructuring needed.

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
| 3 | Agent loop | **the agent-loop protocol**, `serde_json::Value`, `std::fs`, `Command`, `match`-based tool dispatch (Tool trait + registry deferred — see callout) |
| 4 | Modes | `enum` as state machine, exhaustive match, special tools |
| 5 | Streaming | `futures::Stream`, `StreamExt`, SSE parsing, partial JSON |
| 6 | Multi-model | **extracting a trait from two impls**, static vs dynamic dispatch. This is also a natural point to revisit extracting the `Tool` trait from CP3's three concrete tool functions (Rule of Three). |
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
