# Cargo CLI and crate types

## `cargo init` vs `cargo new`

Two commands, almost identical, differing only in *where* they scaffold:

- `cargo new foo` — creates a **new** directory named `foo`, then scaffolds inside it.
- `cargo init` — scaffolds in the **current** directory.

cawir already had `CLAUDE.md`, `learnings/`, and a git repo in place, so `cargo init` was the right call. `cargo new cawir` would have created `cawir/cawir/` — wrong nesting.

Both commands:

- Default to `--bin` (binary crate) if you pass neither `--bin` nor `--lib`.
- Default the package name to the directory name (override with `--name`).
- Detect an existing git repo and skip `git init`. If there's no `.git`, they run `git init` and create a starter `.gitignore`.

Other scaffold flags (all optional):

| Flag | Purpose |
|---|---|
| `--edition <YEAR>` | Pick a specific edition. Defaults to the latest stable the toolchain supports. (See `04-rust-editions.md`.) |
| `--name <NAME>` | Override the package name. |
| `--vcs <VCS>` | Initialize a VCS — `git` (default), `hg`, `pijul`, `fossil`, or `none`. |
| `--registry <REGISTRY>` | Custom registry other than crates.io. Rarely used. |

## `--bin` vs `--lib` — the two crate templates

Only two values. No third.

| Template | Entry file | Purpose | Executable? | `cargo run` works? |
|---|---|---|---|---|
| `--bin` | `src/main.rs` | A program you run | ✅ produces a binary at `target/debug/<name>` | ✅ |
| `--lib` | `src/lib.rs` | Reusable code other crates depend on | ❌ produces a `.rlib` archive for linking | ❌ errors: *"a bin target must be available"* |

### Mental model from TS/Python

- **Binary crate** ≈ a CLI tool, or a Python script you'd `python myscript.py` / `node index.js` directly.
- **Library crate** ≈ an npm package or pip package — code meant to be `import`ed, not directly executed. `serde`, `tokio`, `reqwest` are all library crates.

### Mutual exclusivity

You can't pass both `--bin` and `--lib` to the same `cargo new` / `cargo init` invocation — it errors. Pick one at scaffold time.

## Exotic crate types — configured in Cargo.toml, NOT as scaffold flags

A library crate can emit different artifact formats depending on what it's for. This is **not** a `cargo new` flag — it's a `Cargo.toml` setting:

```toml
[lib]
crate-type = ["cdylib"]
```

| `crate-type` | What it produces | Used for |
|---|---|---|
| `"rlib"` (default) | Rust-only archive. Only other Rust crates can link it. | Normal library crates. |
| `"cdylib"` | C-ABI shared library (`.dylib` on macOS, `.so` on Linux, `.dll` on Windows). | FFI — Rust code callable from C, Python (via PyO3/ctypes), Node (via NAPI), Swift, etc. |
| `"staticlib"` | C-ABI static library (`.a`). | Linking Rust into a C/C++ binary. |
| `"proc-macro"` | A compiler plugin. | Writing custom macros like `#[derive(Serialize)]`. |

`cargo init --lib` always starts you with a plain `rlib` library. Any of the above is a later Cargo.toml edit.

## Mixing `--bin` and `--lib` in one crate

A single crate can be **both** a binary and a library at the same time. You get there by manually creating both files:

```
src/
├── main.rs      ← binary entry point
└── lib.rs       ← library entry point
```

No `cargo new --bin-and-lib` flag exists. Scaffold with one, add the other by hand.

Cargo builds the library first, and `main.rs` can `use cawir::something;` to pull from it.

Why this pattern is common:

1. **Testability.** Integration tests in `tests/*.rs` can only import `pub` items from a crate's **library** — they can't see inside `main.rs`. Putting logic in `lib.rs` makes it testable; `main.rs` becomes a thin wrapper.
2. **Reusability.** Someone (maybe future-you) could embed the library in another Rust program.

Most real-world Rust CLI tools follow this pattern — ripgrep, bat, fd, cargo itself.

For cawir: we scaffolded as `--bin` only. We'll likely grow a `src/lib.rs` alongside once the agent logic is non-trivial (see `03d-crate-layout.md`).
