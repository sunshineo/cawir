# Derive macros and the `derive` feature convention

`#[derive(Deserialize)]` and friends are one of the first "magical" Rust syntaxes you hit. Understanding what actually happens clears up a lot.

**Terminology up front.** `Deserialize`, `Debug`, `Clone`, `Serialize` are **traits** — behavior contracts (see `learnings/06-traits-and-scope.md`). A derive *macro* of the same name exists for each, and its job is to generate an `impl TraitName for YourType` block at compile time. So in `#[derive(Deserialize, Debug)]`:

- The argument (`Deserialize`, `Debug`) is always a **trait name**.
- The struct/enum it's attached to is a **type**.
- The output is one `impl Trait for Type` block per argument.

When this doc says "derive examples: `Debug`, `Serialize`" below, those names refer to traits (with a proc-macro of the same name).

## What `#[derive(...)]` generates

Derive is a **procedural macro** — at compile time, the macro reads your type definition and generates an `impl` block. For `#[derive(Deserialize)] struct Repo { ... }`, the expansion is roughly:

```rust
impl<'de> serde::Deserialize<'de> for Repo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de>
    {
        // ~100 lines of field-by-field Visitor-pattern parsing
    }
}
```

Properties:

- **Runs at compile time, not runtime.** No reflection, no runtime schema lookup.
- **Zero runtime cost** vs hand-writing the impl — the compiler sees identical code.
- **You can inspect the expansion** with `cargo install cargo-expand` then `cargo expand`, or via rust-analyzer's "Expand macro recursively" command.

## Three kinds of Rust macro

| Kind | Syntax | What it does |
|---|---|---|
| Declarative | `macro_rules! foo { (...) => {...} }` → `foo!(...)` | Pattern-match based, substitution-like. Examples: `vec![]`, `println!`. |
| Derive | `#[derive(TraitName)]` on a type | Reads the type definition and emits an `impl TraitName for ThatType` block. The argument is a **trait name**; the attribute sits on a **type** (struct/enum). Example traits: `Debug`, `Serialize`, `Clone`. |
| Attribute / function-like | `#[foo(...)]` on an item, or `foo!{...}` | Arbitrary proc-macro — can read and transform any syntax tree. Examples: `#[tokio::main]`, `sqlx::query!`. |

Derive is the narrowest and most common — specifically for "given a type, provide an impl."

## Built-in vs crate-provided derives

Some derive macros ship with the compiler/stdlib; others come from crates. (Each listed name is a **trait**, with a derive macro of the same name.)

| Always available (stdlib traits) | Require a crate |
|---|---|
| `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `Default`, `PartialOrd`, `Ord` | `Serialize`, `Deserialize` (serde); `Error` (thiserror); `Parser` (clap); etc. |

For crate-provided derives, the trait lives in the crate and the proc-macro that generates the impl usually lives in a separate "derive" sister-crate. Both need to be available at compile time.

## The `"derive"` feature convention

Many crates ship the trait in the main crate and the derive macro in a sister crate, gated behind a feature flag. Example:

```toml
serde = { version = "1", features = ["derive"] }
```

This pulls in `serde_derive` as a transitive dep and re-exports its macros. Without the feature, you get the runtime traits but have to implement them by hand.

**Why the split?** Proc-macros require heavy compile-time dependencies (`proc-macro2`, `syn`, `quote` — together a nontrivial compile). A tiny project using only runtime traits (perhaps implementing `Deserialize` by hand, or in a `no_std` environment where proc-macros are awkward) doesn't want to pay that compile-time cost.

In practice, 99% of serde users want derive. The feature flag is mostly a courtesy.

## Convention across common crates

| Crate | How derive is enabled |
|---|---|
| `serde` | `features = ["derive"]` |
| `thiserror` | Always on (no feature needed) |
| `clap` | `features = ["derive"]` |
| `sqlx` | `features = ["macros"]` |
| `tokio` | `features = ["macros"]` (for `#[tokio::main]`, `#[tokio::test]`) |

The convention is roughly: feature-gated proc-macros where some users don't want them; always-on for crates where derive is the only reasonable way to use them (thiserror).

## Takeaway

- `#[derive(X)]` is syntactic sugar for "please generate an `impl X for ThisType`." The `X` is a trait name; the attribute's target (struct/enum) is a type.
- `X` must be a trait that has a derive macro — either built-in (`Debug`, `Clone`, etc.) or provided by a crate.
- Crate-provided derives often live in a separate `*_derive` crate, pulled in via a `"derive"` or `"macros"` feature flag.
- Generated code is identical to what you'd write by hand; zero runtime overhead.
- `cargo expand` reveals the actual generated code, useful when things go wrong.
