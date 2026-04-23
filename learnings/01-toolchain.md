# Rust toolchain — compared with TypeScript/Node (and a bit of Python)

Anchoring Rust's toolchain concepts against the Node/TS ecosystem, since both share the "package manager + lockfile + version manager" shape.

## The mapping

| Rust | TypeScript/Node equivalent | What it does |
|---|---|---|
| **`rustup`** | **`nvm`** (Node Version Manager) | Installs & switches between compiler/runtime versions |
| **`rustc`** | **`tsc`** (TypeScript compiler) | The compiler itself. You rarely invoke directly. |
| **`cargo`** | **`npm`** + `tsc` + `jest` + `create-*` *all rolled into one* | Build, deps, test runner, scaffolder — one tool |
| **`Cargo.toml`** | **`package.json`** | Project manifest: name, version, deps |
| **`Cargo.lock`** | **`package-lock.json`** / `yarn.lock` | Exact resolved versions for reproducible builds |
| **crate** | **npm package** | A unit of shareable code; lives on **crates.io** (like npmjs.com) |
| **`cargo add foo`** | **`npm install foo`** | Add a dependency |
| **`cargo build`** | **`tsc`** (compile step) | Compile without running |
| **`cargo run`** | **`npm start`** / `ts-node x.ts` | Compile *and* run |
| **`cargo test`** | **`npm test`** (which wraps jest/vitest) | Run tests |
| **`rustfmt`** (`cargo fmt`) | **Prettier** | Opinionated formatter |
| **`clippy`** (`cargo clippy`) | **ESLint** | Linter — but also a teacher |
| **`rust-toolchain.toml`** | **`.nvmrc`** / `"engines"` in package.json | Pin compiler version for this project |
| **stable / beta / nightly channels** | no real Node analogue; closest is **LTS vs Current** | Release trains |

## The three surprises coming from TS/Python land

### 1. One tool, blessed by the core team, does everything

In Node you pick: `npm` vs `yarn` vs `pnpm` vs `bun`. You pick Prettier vs dprint vs Biome. ESLint vs Biome vs Oxlint. Jest vs Vitest vs Mocha vs Node's built-in test runner. Every one of those is a Slack thread.

In Rust: **`cargo` is the package manager. `cargo test` is the test runner. `rustfmt` is the formatter. `clippy` is the linter.** All shipped and maintained by the Rust team. There is no ecosystem debate. You'll see this reflected in every tutorial, every README, every CI file — they all look the same.

**Why it matters for learning:** you can spend 100% of your attention on the language, zero on tool selection.

### 2. Rust compiles to a native binary — Node and Python don't

When you `cargo build`, you get an executable file (`./target/debug/cawir`). No Node runtime required, no Python interpreter required. You can copy that binary to another Mac and run it. That's closer to Java's `javac → .jar` (but `.jar` still needs the JVM) — Rust is closer to **Go** or **C++** in this regard: binary, no runtime.

`cargo run` is just a shortcut that does "build, then execute the binary it just produced."

### 3. `rustup`'s "channels" — a concept that doesn't really exist in TS/Python

Three parallel release trains:

- **stable** — updates every 6 weeks; what 99% of projects use. Use this.
- **beta** — the next stable, ~6 weeks ahead.
- **nightly** — bleeding edge, updates every night. Required only for *unstable features* gated behind `#![feature(...)]` flags. You won't need it.

In TS-land there's no equivalent — there's just "the latest TypeScript." Python has pre-releases but nobody installs `3.14.0a1` casually. `rustup default stable` and you'll never think about this again.

## One more thing: `rustup component add`

When I said earlier "`rustfmt` and `clippy` come with `rustup`" — what actually happens is:

```
rustup component add rustfmt clippy
```

Think of components as *optional pieces of the toolchain*. The core compiler is installed by default; `rustfmt`/`clippy` are opt-in but trivial to add, and most setups add them immediately. There's also things like `rust-analyzer` (the language server — covered in decision 2) and cross-compilation targets (e.g. "let me build a Linux binary from my Mac") available as components.

No real TS/Python analogue — it's like if `npm` could install *its own plugins* from the same registry.
