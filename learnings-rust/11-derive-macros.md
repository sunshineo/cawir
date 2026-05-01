# Derive macros and the `derive` feature convention

`#[derive(Deserialize)]` and friends are one of the first "magical" Rust syntaxes you hit. Understanding what actually happens clears up a lot.

**Terminology up front.** `Deserialize`, `Debug`, `Clone`, `Serialize` are **traits** — behavior contracts (see `learnings-rust/06-traits-and-scope.md`). A derive *macro* of the same name exists for each, and its job is to generate an `impl TraitName for YourType` block at compile time. So in `#[derive(Deserialize, Debug)]`:

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

## Why derive can do this automatically

A natural question, especially coming from Java or JavaScript: when you implement an interface there, you have to write the methods yourself — only you know what `equals()` should compare or how `toString()` should look. So how can Rust's compiler just *generate* an implementation?

The answer: **derive only works for traits whose implementation is a mechanical function of the type's fields**. The proc-macro reads your struct's fields at compile time and emits the deterministic code. There's no creative judgment to make.

### Mechanical traits vs judgment-required traits

| Trait | Derivable? | Why / Why not |
|---|---|---|
| `Debug` | ✅ | "Print every field by name." Mechanical. |
| `Clone` | ✅ | "Clone every field." Mechanical. |
| `PartialEq`, `Eq` | ✅ | "Compare every field for equality." Mechanical. |
| `Hash` | ✅ | "Hash every field in order." Mechanical. |
| `Default` | ✅ | "Default-construct every field." Mechanical. |
| `Serialize` (serde) | ✅ | "Write each field as `\"name\": value`." Mechanical given serde's design. |
| `Deserialize` (serde) | ✅ | "Read each named field, parse by type." Mechanical. |
| `Display` | ❌ | How should it look to a human? Design call. Could be `"{full_name} ({stars} stars)"` or `"#{id}: {full_name}"` — only you know. |
| `Iterator` | ❌ | What does `next()` return? Depends entirely on what you're iterating over. |
| `Drop` | ❌ | Custom cleanup logic — file handles, mutex unlocks, network sockets. Resource-specific. |

For mechanical traits, derive emits the deterministic code. For judgment-required traits, you write the impl by hand — same as implementing a Java interface.

### Recursive composition is what makes it work

The Debug expansion shown above generates code like `f.field("full_name", &self.full_name)` — but how does that know how to print a `String`?

Because **`String` itself implements `Debug`**. The macro doesn't need to know how to print every type — it just calls each field's *own* `Debug` impl. Recursive composition.

If a field's type doesn't implement the trait you're deriving, you get a compile error pointing at that field:

```
error[E0277]: `SomeOtherType` doesn't implement `std::fmt::Debug`
   --> src/main.rs:5:5
    |
3   | #[derive(Debug)]
    |          ----- in this derive macro expansion
4   | struct Repo {
5   |     thing: SomeOtherType,
    |     ^^^^^^^^^^^^^^^^^^^^ `SomeOtherType` cannot be formatted using `{:?}`
help: consider annotating `SomeOtherType` with `#[derive(Debug)]`
```

The fix is exactly what the compiler suggests: derive `Debug` on the inner type. Composition all the way down.

### Java/JS analogs you may have already seen

You've encountered this pattern under different names:

| Language | Equivalent | What gets generated from your fields |
|---|---|---|
| Java | **Lombok's `@Data`, `@EqualsAndHashCode`, `@ToString`** | getters, setters, `equals`, `hashCode`, `toString` |
| Kotlin | **`data class`** | `equals`, `hashCode`, `toString`, `copy()` |
| Python | **`@dataclass`** | `__init__`, `__eq__`, `__repr__` |
| C# | **`record` types** | equality, `ToString`, deconstruction |
| Scala | **`case class`** | equality, hashCode, copy, pattern matching |

Rust's `#[derive(...)]` is functionally the same idea. Three differences worth knowing:

1. **Compile-time only.** Generated code is real Rust, compiled normally. No runtime reflection, no class-loading-time introspection. Lombok also operates at compile time. Python's `@dataclass` runs at class-definition time using runtime reflection (which is slightly slower).
2. **Fully type-checked.** Generated code passes through the same type checker as code you'd write by hand. Bad derives produce compile errors, not runtime surprises.
3. **Extensible by anyone.** Any crate can define new derive macros. Lombok is a single library with a fixed set of macros built into it. In Rust, `serde`, `thiserror`, `clap`, `tokio`, and arbitrarily many other crates each define their own derives.

### Why Java and JS don't have this natively

**Java has it via Lombok**, which hooks into Java's annotation processor APIs. Powerful, but feels like an add-on, and IDE support has rough edges.

**Kotlin / C# / Scala / Python** built equivalents into the language (`data class`, `record`, `case class`, `@dataclass`). Rust did the same, but went further by making derive open-ended — any crate can define new ones, not just the language standard.

**JavaScript / TypeScript** have decorators, but they typically run at runtime and require interpreter cooperation. Different model — closer to Python decorators than Rust derives.

### When to write the impl by hand

Whenever the trait requires a decision the macro can't make:

```rust
struct Repo { full_name: String, stargazers_count: u32, /* ... */ }

// Display — how should this look in user-facing strings?
// Only you know.
impl std::fmt::Display for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} ({} stars)", self.full_name, self.stargazers_count)
    }
}
```

`Display` isn't derivable because there's no obvious mechanical rule. `Debug`-style "print every field with its name" works for debugging but isn't appropriate for user-facing output. The choice — what fields to show, how to format them, what punctuation/separators to use — is yours.

You'd write this exactly the way you'd write a Java `toString()` or a JS `toJSON()` — manually, expressing your design choice.

### One-line summary

Derive works because **the trait's implementation is fully determined by the type's structure**. The proc-macro can compute it at compile time by walking the fields. Mechanical traits get derived; judgment-required traits get hand-written.

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
- **Derive only works for traits whose implementation is mechanical from the struct's fields.** Judgment-required traits (`Display`, `Iterator`, custom domain traits) need hand-written impls — same as implementing a Java interface.
- Closest analogs in other languages: **Lombok** (Java) and **`@dataclass` / `data class` / `record` / `case class`** in Python / Kotlin / C# / Scala.
- `cargo expand` reveals the actual generated code, useful when things go wrong.
