# cawir — Coding Agent Written in Rust

A minimal, educational coding agent written in Rust. Built incrementally as a learning vehicle with two equal goals:

1. **Learn Rust** — by implementing real features, not toy examples
2. **Learn how coding agents work** — by building one from scratch, not using one

The target is *not* to compete with Claude Code or Cursor, but to create a small, readable agent that its author fully understands.

## What cawir Does

cawir is a conversational coding agent that:
- **Talks with Claude** via the Anthropic API
- **Reads files and directories** from your project (`read_file`, `list_files`)
- **Writes files** with explicit per-write approval (`write_file`)
- **Executes shell commands** (planned for next release)
- **Loops on tool calls** — Claude can request tools, cawir executes them, Claude sees results and continues until providing a text answer

Example conversation:
```
cawir> read Cargo.toml and tell me the dependencies
[Claude decides to read_file → cawir executes → Claude sees result → Claude responds]
claude: Here are your dependencies: tokio for async runtime...
```

## Project Status

**Current focus:** Checkpoint 3 (Agent loop) — building the core tool-execution and looping infrastructure.

**Latest work:**
- ✅ Advertise and execute read-only tools (`read_file`, `list_files`)
- ✅ Execute tool calls in a loop until Claude returns text
- ✅ Handle tool failures gracefully (return errors to Claude instead of crashing)
- ✅ Add first mutating tool (`write_file`) with inline approval
- 🔄 Next: `shell` tool with inline approval (Checkpoint 3i)

See [`docs/status.md`](docs/status.md) for detailed progress and [`docs/roadmap.md`](docs/roadmap.md) for the full plan.

## Architecture

cawir is organized into functional layers:
- **Surface** — REPL and user I/O
- **Core engine** — Agent loop and session state
- **External** — HTTP, model providers, file I/O
- **Policy** — Permission modes and hooks (future)
- **Capabilities** — Tool registry and execution (future)

The target architecture is documented in [`docs/architecture.md`](docs/architecture.md).

## Project Organization

### Source code
- **`src/main.rs`** — REPL entry point
- **`src/lib.rs`** — Agent loop, tool execution, Claude communication
- **`src/session.rs`** — Message types and conversation state
- **`src/error.rs`** — Typed error enum

### Documentation
- **`docs/status.md`** — Current implementation state and recent progress
- **`docs/roadmap.md`** — Checkpoint sequence and scope
- **`docs/architecture.md`** — Target design and extension seams
- **`learnings-rust/*.md`** — Durable Rust notes (ownership, traits, async, etc.)
- **`learnings-agent/*.md`** — Durable coding-agent design notes

### Collaboration
- **`CLAUDE.md`** — Project collaboration rules for AI assistants (symlinked as `AGENTS.md`)

## How to Run

### Prerequisites
- Stable Rust installed via [rustup](https://rustup.rs)
- An Anthropic API key from [console.anthropic.com](https://console.anthropic.com)

### Quickstart
```bash
echo 'ANTHROPIC_API_KEY=sk-ant-...' > .env
cargo run
```

You can also export `ANTHROPIC_API_KEY` in your shell instead. Shell environment
variables win over values from `.env`.

Then type your questions:
```
cawir> what files are in this directory?
[Claude runs list_files; displays result]
claude: I see the following files...

cawir> read Cargo.toml and summarize the dependencies
[Claude runs read_file; displays result]
claude: This project uses tokio for async...
```

### Commands
- `/exit` — Quit cawir
- `/help` — Show help
- Any other text — Sent to Claude as a user message

## Development

### Running tests
```bash
cargo test
```

### Code style
```bash
cargo fmt      # Format code
cargo clippy   # Lint suggestions
cargo build    # Build
```

### Learning philosophy

This project is designed as a teaching vehicle. Each code change:
- Introduces one major new Rust concept (async/await, traits, error handling, etc.)
- Solves a real problem, not a toy example
- Is discussed and documented in `learnings-rust/` before committing

Rust-learning discussions are distilled into `learnings-rust/*.md`. Coding-agent design discussions go into `learnings-agent/*.md`.

## Key Dependencies

- **`tokio`** — Async runtime
- **`reqwest`** — HTTP client
- **`serde` + `serde_json`** — JSON serialization
- **`thiserror`** — Typed error handling

## Next Steps

See the [roadmap](docs/roadmap.md) for the full development plan:

- **Checkpoint 3i (Next)** — Add `shell` tool with inline approval
- **Checkpoint 4** — Permission modes (`/mode plan`, `/mode bypass`)
- **Checkpoint 5** — Streaming responses
- **Checkpoint 6** — Multi-model support (OpenAI)
- **Checkpoint 7+** — Hooks, additional providers, session resume

## Design Decisions

- **Hand-rolled HTTP** — No model SDK. The wire protocol is part of what we're learning.
- **Inline approval first** — Mutating tools are gated by direct REPL prompts before a full permission system lands.
- **Concrete before abstract** — No traits until Rule of Three (three similar implementations) creates real extraction pressure. Early checkpoints stay concrete and readable.
- **Fail gracefully** — Tool execution errors and user denials are returned to Claude as tool-result blocks, not crash signals.

## License

This is a learning project. Feel free to fork, reference, or use as a learning resource.

## References

- [Anthropic API docs](https://docs.anthropic.com)
- [Rust async/await](https://rust-lang.github.io/async-book/)
- [The Rust Book](https://doc.rust-lang.org/book/)
