# LSP, rust-analyzer, and why Rust needs VS Code extensions (TS/Python/Java compared)

## The pattern every language follows now

Modern editors don't understand languages themselves. They offload "smart" features to two protocols:

- **LSP** (Language Server Protocol) — autocomplete, go-to-definition, rename, inline errors, hover docs. A **language server** is a separate program the editor talks to over JSON-RPC.
- **DAP** (Debug Adapter Protocol) — breakpoints, stepping, variable inspection. A **debug adapter** is a separate program the editor talks to.

So for *every* language in VS Code, there's really three things: the editor, the LSP server, and the debugger. What changes is **how much of that is bundled vs. installed**.

## Why TS "just works," Python kind-of-did, and Java is a mess

| Language | Bundled in VS Code? | What you actually installed | Why |
|---|---|---|---|
| **TypeScript** | ✅ Fully. `tsserver` (the LSP server) ships *inside* VS Code. | Nothing. | VS Code is **written** in TypeScript, by Microsoft, who also maintains TypeScript. Special case. |
| **JavaScript** | ✅ Fully. Same `tsserver` handles JS too. | Nothing. | Same as above. |
| **Python** | ❌ *Not* bundled — you almost certainly installed an extension. | **Python extension** (Microsoft). It bundles the Pylance LSP server + `debugpy` debugger. | Usually installed via a popup: *"Install the recommended extension for Python?"* the first time you opened a `.py` file. Only **syntax highlighting** works without it. |
| **Java** | ❌ Not bundled, famously heavy. | **Extension Pack for Java** — ~5 extensions (Language Support for Java, Debugger for Java, Test Runner, Maven, Project Manager). Plus a JDK. | Java's tooling is large (JDK + Maven/Gradle + JUnit + debugger), so each role gets its own extension. |
| **Rust** | ❌ Not bundled. | **rust-analyzer** extension + **CodeLLDB** extension. | Same shape as Python — one LSP extension + one debugger — just not pre-suggested by VS Code. |
| **Go** (reference) | ❌ Not bundled. | **Go** extension. Bundles `gopls` LSP + `delve` debugger. | Same shape. |

**Bottom line:** Rust is roughly like **Go or Python**, not like Java. Two extensions, done.

## What "rust-analyzer" actually is — three things sharing a name

1. **rust-analyzer (the project)** — an open-source project building a Rust language server. Lives at github.com/rust-lang/rust-analyzer. Stewarded by the Rust org.
2. **`rust-analyzer` (the binary)** — the compiled language server program. ~30 MB executable that speaks LSP over stdin/stdout. This is *the actual thing* doing the Rust analysis.
   - Two install paths: (a) `rustup component add rust-analyzer`, or (b) auto-downloaded by the VS Code extension. Either works; the extension defaults to downloading its own copy.
3. **`rust-analyzer` (the VS Code extension)** — a thin JavaScript shim that:
   - Registers itself as the handler for `.rs` files.
   - Downloads the binary if not present.
   - Launches the binary as a subprocess and pipes LSP messages between VS Code and it.
   - Adds Rust-specific commands (e.g. "rust-analyzer: Expand Macro Recursively").

You **install the extension**, and it transparently manages the binary.

## What "CodeLLDB" is — debugger wiring

1. **LLDB** — a debugger from the LLVM project. Apple bundles it with macOS (via the Xcode Command Line Tools) at `/usr/bin/lldb`. The `rust-lldb` script in `~/.cargo/bin/` wraps it with Rust pretty-printers.
2. **CodeLLDB** — a VS Code extension that speaks **DAP** on one side and drives **LLDB** on the other. Same pattern as rust-analyzer: editor speaks a standard protocol, extension translates.

Why this specific extension (vs. Microsoft's C/C++ extension, which also drives LLDB)?

- Understands Rust's pretty-printers: `Vec` shows as its elements, not raw memory.
- Handles `Option<T>` and `Result<T, E>` nicely in the Variables panel.
- Officially recommended in the rust-analyzer docs.

## One gotcha

There's an old, deprecated extension called **"Rust"** (by the rust-lang team), based on an older server called `RLS` (Rust Language Server). **Do not install it.** Superseded by rust-analyzer in 2022, no longer maintained.

When searching "rust" in VS Code's Extensions panel, pick the one called exactly **`rust-analyzer`**, publisher **The Rust Programming Language**.
