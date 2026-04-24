# Traits and the "trait must be in scope" rule

Traits are Rust's primary abstraction mechanism. Not a class — closer to a Java `interface`, with extra powers.

## What a trait is

A set of method signatures that types implement. Describes behavior, not data.

```rust
trait Greet {
    fn greet(&self) -> String;
}

struct Person { name: String }

impl Greet for Person {
    fn greet(&self) -> String {
        format!("Hi, {}!", self.name)
    }
}
```

Three pieces:

- `trait Greet { ... }` — declares the trait and its method signatures.
- `struct Person` — a plain data type.
- `impl Greet for Person { ... }` — **the gluing mechanism.** Connects the trait to the type with method bodies.

Without the `impl` block, `Person` has no relationship to `Greet`.

## Mental model by language

| Language | Same idea called |
|---|---|
| Java / C# / TypeScript | `interface` |
| Go / Swift | `interface` / `protocol` |
| Python | ABCs / Protocols |
| Rust | `trait` |

Differences from Java interfaces:

- Traits can have **default method bodies** (like Java 8+ default methods).
- You can **implement a trait for a type you didn't define** (the "orphan rule" allows this as long as you own either the trait or the type). This is the key superpower.
- Traits are how Rust does **polymorphism** — no class inheritance exists in the language.
- Some traits are **markers** — no methods, just a label (e.g., `Send`, `Sync`, `Copy`).

## The trait-in-scope rule

**To call a trait's methods on a type, the trait must be imported at the call site.**

```rust
use std::io::Write;        // MUST have this
use std::io::stdout;

fn main() {
    stdout().flush();      // calls Write::flush — only works if Write is in scope
}
```

Without `use std::io::Write;`, the compiler errors:

```
error[E0599]: no method named `flush` found for struct `Stdout` in the current scope
help: items from traits can only be used if the trait is in scope
help: add `use std::io::Write;`
```

## What `use SomeTrait` actually means

It does **not** mean "apply SomeTrait to a specific type."

It means: **"Enable SomeTrait's methods on any type that already implements SomeTrait, anywhere in this file."**

Two things must combine:

| Thing | Provided by |
|---|---|
| `impl Trait for Type` block | Whichever crate owns the impl (stdlib, third-party, or your code) |
| `use Trait;` statement | Your `use` at the top of the file |

The `impl` block is what makes a type a "type that implements Trait." Your `use` is the permission slip to call those trait methods from your code. Both are required.

## Why the rule exists: method name collisions

Different traits can define methods with the same name. Rust uses scope to disambiguate.

This happens in the real stdlib:

| Trait | What it writes | Example methods |
|---|---|---|
| `std::io::Write` | Raw bytes (`&[u8]`) | `write`, `write_all`, `flush` |
| `std::fmt::Write` | UTF-8 text (`&str`) | `write_str`, `write_char` |
| `tokio::io::AsyncWrite` | Async bytes | `poll_write`, ... |

Different types implement different ones:

| Type | `io::Write`? | `fmt::Write`? |
|---|---|---|
| `Stdout` | ✅ | ❌ |
| `File` | ✅ | ❌ |
| `String` | ❌ | ✅ |
| `fmt::Formatter` | ❌ | ✅ |

If `std::io::Write` and `std::fmt::Write` were both in scope, `x.write(...)` would be ambiguous. The scope-based rule forces you to pick one (or use aliases).

## Multiple same-named traits — use aliases

If you really need both:

```rust
use std::io::Write as IoWrite;
use std::fmt::Write as FmtWrite;
```

Now the two Write traits have different names in your file. No ambiguity.

More commonly, you just import whichever one you need in each file — rarely does one file need both.

## Why Rust has this rule when Java doesn't

- **Java:** only the class's owner can add methods. Nobody can bolt `.serialize()` onto `String` after the fact.
- **Rust:** any crate in your dependency graph can add trait impls across existing types (subject to the orphan rule). `serde` can add `Serialize`. `askama` can add `Render`. This flexibility is powerful but creates collision risk.

The trait-in-scope rule says: **methods from external traits only take effect if you opt in.** No surprise behavior from deep dependencies.

## The prelude — why you don't hit this for common traits

A set of traits is auto-imported into every Rust module (the "prelude"):

- `Iterator` → `.map()`, `.filter()`, `.collect()` always work
- `Clone`, `Copy`, `Debug` — common derives
- `Into`, `From` — common conversions
- `Drop` — destructors

I/O traits (`Read`, `Write`, `BufRead`, `Seek`) are **not** in the prelude. That's why I/O is where beginners first hit "trait not in scope."

## How to find what a type implements

| Method | How |
|---|---|
| **rust-analyzer hover** | In VS Code, hover over a method — rust-analyzer shows the trait it comes from. |
| **docs.rust-lang.org / docs.rs** | Every type's doc page has a "Trait Implementations" section. `cargo doc --open` generates it locally for your crate's deps. |
| **Compiler error messages** | If you forget an import, the error tells you exactly which `use` to add. |

## `use` syntax for traits (and other items)

```rust
use std::io::Write;               // import one trait
use std::io::{Write, Read};       // import two
use std::io::{self, Write};       // import the module `io` AND the trait Write
use std::io::Write as IoWrite;    // alias
use std::io::*;                   // glob — imports everything (generally avoid)
```

`self` inside `{...}` means "the module itself." `use std::io::{self, Write}` is equivalent to `use std::io; use std::io::Write;`.

## Takeaway

- `impl Trait for Type` is the fundamental gluing mechanism of Rust.
- `use SomeTrait` enables that trait's methods in your file, for any type that already implements it.
- Multiple traits can share a name (disambiguated by path).
- Common traits are in the prelude; I/O and many others are not.
- The compiler tells you what to import when you forget.
