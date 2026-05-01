# Rust editions

Every 2–3 years, Rust publishes a new **edition** — 2015, 2018, 2021, 2024. Each edition can make **backward-incompatible** syntactic / lint changes to the language.

The clever part: editions are **per-crate**, not per-toolchain. A 2024-edition crate can depend on a 2018-edition crate; the compiler handles both. This is how Rust has evolved for a decade without ever having a "Python 2 / Python 3"–style ecosystem split.

## The one-sentence model

**An edition is a syntax-and-defaults boundary, not a compiler boundary.** Every crate picks an edition in its `Cargo.toml`; the *same* compiler (`rustc`) then applies edition-specific parsing rules, prelude contents, and warn-to-error upgrades to that crate's source code.

Crucially: the **bytecode it produces**, the **stdlib**, and the **type system** don't vary by edition. Two crates on different editions compile to compatible artifacts and can link together. That's the whole trick.

## What an edition CAN change

| Category | Example |
|---|---|
| **New reserved keywords** | `async`, `await`, `dyn` became keywords in 2018. `gen` reserved in 2024. |
| **Syntax changes** | Path resolution: `::foo::bar` in 2015 → `crate::foo::bar` in 2018+. |
| **Prelude contents** | What's auto-imported into every module. Each edition has its own prelude — 2021 added `TryInto`, `TryFrom`, `FromIterator`. |
| **Default behaviors** | 2021 changed closure capture: closures now capture only the fields they use, not the whole struct. |
| **Lint level upgrades** | A warning in edition N can become an error in edition N+1. |
| **Operator/trait coverage** | 2021 added `IntoIterator for [T; N]`, so `for x in [1,2,3]` Just Works. Was a compile error in 2018. |

## What an edition CANNOT change

| Category | Why not |
|---|---|
| **Stdlib behavior** | stdlib evolves on its own cadence, version by version. A new stdlib method appears in a rustc release regardless of edition. |
| **Type inference/checking rules** | These are universal — breaking them would break inter-crate linking. |
| **Performance / codegen** | Produced machine code is identical across editions for equivalent source. |
| **Crate binary format** | A 2015 `.rlib` and a 2024 `.rlib` are the same format. |
| **MSRV** (minimum supported rustc) | Editions don't set the minimum compiler version — that's `rust-version` in `[package]`. |

This separation is why you can bump an edition without touching semantic behavior.

## Mechanics: how rustc actually does this

Internally, `rustc` is essentially several parsers and several sets of defaults held in one binary. When compiling your crate, it reads `edition = "2024"` from `Cargo.toml` and flips internal switches:

- Which tokens are keywords (so `async` tokenizes as a keyword, not an identifier)
- Which prelude to auto-import
- Which lints are on / at what severity
- Which parsing ambiguities resolve which way

Each crate is compiled in isolation with its own edition settings, then the resulting `.rlib` artifacts link together at the end. From the linker's perspective, there's no edition — just Rust code.

## Concrete examples from each edition

### 2015 — the original

Nothing to migrate from. The things 2018+ looks weird compared to:

```rust
extern crate serde;                    // needed, even after adding serde to Cargo.toml
use ::std::collections::HashMap;       // absolute path with leading ::
```

### 2018 — the module/syntax cleanup

```rust
// 2015
extern crate serde;
use ::std::collections::HashMap;

fn handler(req: Box<Future<Item=(), Error=()>>) { ... }   // bare trait type

// 2018
use std::collections::HashMap;  // extern crate no longer needed; paths cleaner
use crate::my_module::Thing;    // `crate::` prefix is canonical

fn handler(req: Box<dyn Future<Output=()>>) { ... }       // `dyn` keyword required
async fn fetch() { ... }                                   // async/await
let y = result?;                                           // ? extended to Option
```

Biggest shift: **`async`/`await` became reserved keywords and the `dyn` syntax for trait objects became required.** Both are ubiquitous in modern Rust.

### 2021 — ergonomics tune-up

```rust
// 2018 — error: [i32; 3] doesn't implement IntoIterator
for x in [1, 2, 3] { println!("{}", x); }

// 2021 — just works
for x in [1, 2, 3] { println!("{}", x); }
```

Plus:

- **Disjoint closure capture** — closures capture only the fields they use, not the whole struct
- **Panic macro consistency** — `panic!` behaves the same in any context
- **`TryFrom`/`TryInto` in the prelude** — so you don't need `use std::convert::TryFrom`
- **Cargo resolver v2 by default** — smarter transitive feature handling

### 2024 — latest

The 2024 edition is a grab-bag of smaller things — no single dramatic shift:

- `gen` is now a reserved keyword for future generators
- **Precise capture in `impl Trait`** — lets return types specify exactly which lifetimes they capture, fixing long-standing soundness footguns
- **Temporary scopes in `if let`** — the temporary no longer outlives the `if let` body, fixing a surprising lifetime extension
- **Edition-gated unsafe requirements** — certain operations (setting some env vars, calling some attributes) that used to be implicitly unsafe now require explicit `unsafe` blocks
- New prelude additions (`Future`, `IntoFuture`)

For cawir: we use 2024 because it's the latest. You won't hit most of these corners in normal application code.

## Migration: how you actually bump an edition

Bumping editions is usually one command plus a `Cargo.toml` edit:

```bash
cargo fix --edition        # rustc applies automated fixes
# Then, in Cargo.toml:
edition = "2024"           # flip from the old version to the new
cargo build                # confirm it still compiles
```

`cargo fix --edition` runs the compiler with a special lint group that suggests idiomatic 2024 patterns for every 2021-style construct, then rewrites your source in place. The remaining non-automatable changes are compile errors with clear messages, and you fix them by hand.

This is very different from the Python 2→3 experience, where migration was a months-long, repo-wide, often-manual process. Rust edition migrations are usually done in an afternoon for a moderate codebase.

## The crucial insight: cross-edition interop

**You can depend on crates from any edition, regardless of your own edition.** The world's Rust code is a patchwork of editions — most of crates.io is a mix of 2018 / 2021 / 2024 today. The compiler compiles each crate against its declared edition, then links them.

Practical consequences:

- When you `cargo add reqwest`, you don't care what edition `reqwest` declares internally.
- Upgrading your own crate's edition doesn't force anyone downstream to upgrade.
- A crate can hold a lower edition forever if it wants, and it won't rot — the compiler will keep supporting it indefinitely.

## Why Rust can do this and other languages can't

Three design decisions made editions possible:

1. **No runtime.** Rust compiles to native code. There's no interpreter that has to choose "which language version am I?" at runtime — every crate decides at compile time and produces an artifact that's language-version-neutral.
2. **Strong module/crate boundaries.** Each crate is compiled as a unit. Edition affects a compilation unit, not the whole build.
3. **A compiler team committed to non-breakage.** Rustc maintains all old editions simultaneously. 10-year-old code in edition 2015 still compiles today with the current rustc. This has costs — old edge cases accumulate — but it's a core principle.

Compare to Python: the runtime *is* the interpreter, so you can't mix 2 and 3 in one process. Compare to C: there's no equivalent concept at all — `-std=c11` vs `-std=c17` are compiler flags, not per-file declarations, and mixing is messy.

## Should you ever pick an older edition?

**Almost never, for new crates.** Pick the latest edition supported by your `rust-toolchain` / `rust-version` constraint. For cawir with current stable rustc, that's 2024.

Reasons to pin older:

- You need to support a very old Rust compiler for some reason (the "MSRV" constraint).
- You're contributing a PR to a crate that's still on an older edition and you shouldn't change it in your PR.

That's basically it.

## Timeline context

The first edition, 2018, was Rust's first big test of "can we evolve a stable language without breaking ecosystem trust?" It worked. Shepherded by the Rust lang team (Niko Matsakis, Aaron Turon, and others at the time), the migration was smooth enough that editions have since become routine. It's one of the more copied ideas in modern language design — Go is wrestling with similar concepts now, and TypeScript's `strict` family of flags is philosophically adjacent.
