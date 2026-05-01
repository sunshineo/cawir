# Prelude, `Debug`, and hidden imports

Rust code often uses names like `Debug`, `From`, `String`, `Vec`, `Result`, `Ok`, and `Err` without explicit `use` statements.

That can feel strange coming from languages where every imported name is visible at the top of the file.

The reason is the Rust prelude.

## The prelude

The prelude is a set of very common standard-library names that Rust automatically brings into scope in every module.

It includes names such as:

```rust
String
Vec
Box
Option
Some
None
Result
Ok
Err
Clone
Copy
Debug
Default
Drop
Iterator
From
Into
```

That is why code can write:

```rust
#[derive(Debug)]
```

without:

```rust
use std::fmt::Debug;
```

And why code can rely on `From` conversions without:

```rust
use std::convert::From;
```

The names are already in scope.

## `Debug`

`Debug` is a standard library trait:

```rust
std::fmt::Debug
```

A trait is like an interface or protocol: it defines behavior a type can support.

`Debug` means:

> This type can be printed for programmer-facing debugging.

It is used by formatting placeholders like:

```rust
println!("{:?}", value);
println!("{:#?}", value);
```

Error types almost always implement `Debug` because Rust's standard error trait requires it.

This:

```rust
#[derive(Debug)]
```

tells Rust to generate a default `Debug` implementation for the type.

## `Error` vs `Debug`

In cawir's `error.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    // ...
}
```

`Debug` comes from the standard library prelude.

`Error` comes from:

```rust
use thiserror::Error;
```

That means these two names in the same `derive` list come from different places:

```rust
#[derive(Debug, Error)]
```

| Name | Comes from | Purpose |
|---|---|---|
| `Debug` | Rust standard library | Generate debug printing |
| `Error` | `thiserror` crate | Generate standard error behavior and display formatting |

This is one reason macro-heavy Rust can feel harder to trace: the names in a single attribute can come from different sources.

## `From`

`From` is also a standard library trait:

```rust
std::convert::From
```

It represents conversions:

```rust
impl From<A> for B
```

means:

> Rust knows how to create `B` from `A`.

Example:

```rust
let s = String::from("hello");
```

works because the standard library provides a conversion from `&str` to `String`.

In cawir, `thiserror` uses `#[from]` to generate:

```rust
impl From<reqwest::Error> for Error
```

That lets `?` convert `reqwest::Error` into cawir's application `Error`.

## Language syntax can use traits

Rust often defines language syntax in terms of traits.

For example:

```rust
a + b
```

usually uses:

```rust
std::ops::Add
```

Similarly, for ordinary `Result`, the `?` operator uses conversion traits to turn one error type into another before returning early.

That can feel odd at first:

> A language operator is using a standard library trait.

But this is a common Rust design pattern. It makes language syntax extensible to user-defined types.

## Fully qualified paths

When hidden imports feel confusing, many names can be written with full paths.

Instead of:

```rust
#[derive(Debug)]
```

you can write:

```rust
#[derive(std::fmt::Debug)]
```

Instead of:

```rust
impl From<reqwest::Error> for Error
```

you can write:

```rust
impl std::convert::From<reqwest::Error> for Error
```

Rust code usually uses the shorter forms for common prelude names, but the longer forms are useful for learning and disambiguation.

## The annoying but useful mental model

When a name appears without a module path:

```rust
Debug
From
String
Vec
Option
Result
Iterator
```

check three places:

1. Was it defined in this module?
2. Was it imported with `use`?
3. Is it part of the Rust prelude?

That third bucket is invisible in the source file, which is convenient for experienced Rust users and annoying while learning.

## The takeaway

- `Debug` is a standard trait: `std::fmt::Debug`.
- `From` is a standard trait: `std::convert::From`.
- Both are available without imports because of the Rust prelude.
- `Error` in `#[derive(Debug, Error)]` comes from `thiserror`, not the prelude.
- Rust language features often rely on standard traits.

Rust is explicit about types and ownership, but it can be compact around traits, derives, and prelude names. When the code feels too compact, expanding names to full paths can make it easier to trace.

