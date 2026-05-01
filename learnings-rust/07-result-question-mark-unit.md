# `Result`, `?`, and the unit type `()`

Rust's error handling: no exceptions, no `null`, no `undefined`. Errors are regular values in a `Result` enum. The `?` operator is syntactic sugar for propagating errors up the call stack.

## The unit type `()`

Rust's equivalent of `void`. A zero-sized type with exactly one value, also spelled `()`.

```rust
fn greet() { println!("hi"); }     // returns ()
let nothing: () = ();              // the type AND the value
```

Used when a function has no meaningful return value. Unlike Java's `void`, `()` is a real type you can pass around — you just rarely want to.

## `Result<T, E>` — an enum with two variants

```rust
enum Result<T, E> {
    Ok(T),    // success — carries the successful value
    Err(E),   // failure — carries the error value
}
```

Two type parameters: `T` (success type), `E` (error type).

| Function signature | Means |
|---|---|
| `fn parse(s: &str) -> Result<i32, ParseIntError>` | Returns `i32` on success, `ParseIntError` on failure |
| `fn read() -> io::Result<String>` | Returns `String` on success, `io::Error` on failure |
| `fn flush() -> Result<(), io::Error>` | Returns `()` (nothing meaningful) on success, `io::Error` on failure |

## Type aliases for `Result`

Stdlib and crates define custom `Result` aliases to save typing:

| Alias | Expands to |
|---|---|
| `io::Result<T>` | `Result<T, io::Error>` |
| `fmt::Result` | `Result<(), fmt::Error>` |
| `serde_json::Result<T>` | `Result<T, serde_json::Error>` |
| `anyhow::Result<T>` | `Result<T, anyhow::Error>` |

Purely convenience — semantically identical to the long form.

## `Ok(())` — why the double parens

Two unrelated concepts that happen to both use parens:

- `Ok(value)` — the success-variant constructor of `Result`. Wraps a successful value.
- `()` — the unit type / unit value.

When the success type is `()`, `Ok(...)` wraps the unit value:

```rust
Ok( () )   // Ok of the unit value
```

Whitespace-stripped: `Ok(())`. The outer parens are the `Ok(...)` constructor; the inner `()` is the value being wrapped.

Other examples for contrast:

| Function returns | Success expression |
|---|---|
| `Result<i32, E>` | `Ok(42)` |
| `Result<String, E>` | `Ok(String::from("hi"))` |
| `Result<(), E>` | `Ok(())` |
| `Result<Vec<u8>, E>` | `Ok(vec![1, 2, 3])` |

## The `?` operator

Short definition: **"Unwrap success or propagate error."**

```rust
// Without ?:
let line = match io::stdin().read_line(&mut buf) {
    Ok(n) => n,
    Err(e) => return Err(e),
};

// With ?:
let line = io::stdin().read_line(&mut buf)?;
```

The `?` expands (roughly) to that `match` with early return. If the `Result` is `Ok(x)`, unwrap to `x` and continue. If it's `Err(e)`, return `Err(e)` from the enclosing function immediately.

## Where you can use `?`

`?` requires the **enclosing function** to return a type compatible with the error. The common compatible types:

- `Result<T, E>` — propagates errors that convert into `E`
- `Option<T>` — propagates `None`

You **cannot** use `?` inside a function returning `()`. That's why changing `fn main()` to `fn main() -> io::Result<()>` was necessary to use `?` in main.

## `fn main() -> io::Result<()>`

Rust's `main` can return either:

- `()` — the default. No errors propagated out.
- A type implementing `Termination` — most usefully, `Result<(), E>` where `E: Debug`. Errors are printed via `Debug` and the program exits non-zero.

```rust
fn main() -> io::Result<()> {
    do_something()?;     // if this fails, main returns Err — program exits non-zero
    Ok(())               // success
}
```

If `main` returns `Err(e)`, Rust prints `Error: <debug of e>` and exits with status code 1. No panic, no stack trace — clean error exit.

## `?` also does automatic error conversion

If the `?`'d `Result` has an `Err` of type `E1`, and the enclosing function returns `Result<_, E2>`, `?` will try to convert `E1` into `E2` via the `From` trait:

```rust
fn thing() -> Result<(), MyError> {
    std::fs::read("x")?;    // this returns io::Error
    // ?'s implicit conversion: io::Error → MyError via `impl From<io::Error> for MyError`
    Ok(())
}
```

This is why `thiserror`'s `#[from]` attribute is useful — it derives those `From` impls automatically:

```rust
#[derive(thiserror::Error, Debug)]
enum MyError {
    #[error("I/O: {0}")]
    Io(#[from] io::Error),    // #[from] generates `impl From<io::Error> for MyError`
}
```

With that, `?` on an `io::Result` inside a `Result<_, MyError>`-returning function just works.

## Compared to other languages

| Language | How errors are signaled |
|---|---|
| Java / Python / JS | Exceptions thrown at runtime |
| Go | Multiple return values: `result, err := foo()` + manual check |
| Rust | `Result` enum, explicit in the type; `?` propagates |
| Haskell | `Either` (equivalent to Rust's `Result`) or `Maybe` |

Rust's approach is closer to Haskell's `Either`. Every fallible function has its error path in its type signature — no invisible control flow. The `?` operator makes propagation ergonomic without hiding it.

## Takeaway

- `()` is the unit type — "nothing," used as the success type when no value is meaningful.
- `Result<T, E>` has two variants: `Ok(T)` and `Err(E)`.
- `Ok(())` = "Ok wrapping the unit value."
- `?` is shorthand for "unwrap success or return err." Requires a compatible return type on the enclosing function.
- `fn main() -> io::Result<()>` makes `?` usable in main and gives a clean exit-on-error path.
- `?` auto-converts errors via `From` — `thiserror`'s `#[from]` generates those conversions for you.
