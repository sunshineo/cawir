# `thiserror`, `From`, and the `?` operator

Checkpoint 2f replaced `Box<dyn Error>` with a real application error enum:

```rust
#[derive(Debug, Error)]
pub enum Error {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}
```

This compact syntax hides several pieces of Rust machinery.

## First ignore the attributes

The core Rust enum variant is this:

```rust
Http(reqwest::Error)
```

That means:

> The app's `Error` enum has a variant named `Http`, and that variant stores one `reqwest::Error`.

A value of this variant looks like:

```rust
Error::Http(some_reqwest_error)
```

This part is plain Rust. No macro magic yet.

## `#[error("http error: {0}")]`

This is syntax understood by the `thiserror` crate.

It is not a function. It is an attribute read by the `#[derive(Error)]` macro.

It says:

> When this variant is printed with `{}`, format it as `http error: ...`.

The `{0}` means:

> The first field of this tuple-like enum variant.

Since the variant is:

```rust
Http(reqwest::Error)
```

`{0}` refers to the stored `reqwest::Error`.

So if the inner error displays as:

```text
error sending request
```

then the app error displays as:

```text
http error: error sending request
```

For named-field variants, `thiserror` can use field names instead:

```rust
#[error("anthropic api error {status}: {body}")]
Api {
    status: reqwest::StatusCode,
    body: String,
}
```

## `#[from]`

This is also syntax understood by `thiserror`.

It is not a Rust keyword and not a function. It is a helper attribute.

This:

```rust
Http(#[from] reqwest::Error)
```

tells `thiserror`:

> Generate a conversion from `reqwest::Error` into my app's `Error`.

The generated code is roughly:

```rust
impl std::convert::From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Error::Http(value)
    }
}
```

This is what "a `From` implementation" means: an implementation of Rust's standard `From<T>` conversion trait for a specific pair of types.

## What is `std::convert::From`?

`From` is a standard library trait for type conversion.

Conceptually:

```rust
trait From<T> {
    fn from(value: T) -> Self;
}
```

Example:

```rust
let s = String::from("hello");
```

This works because the standard library implements:

```rust
impl From<&str> for String
```

In cawir, `thiserror` generates:

```rust
impl From<reqwest::Error> for Error
```

which means Rust knows how to convert:

```rust
reqwest::Error
```

into:

```rust
cawir::Error
```

by wrapping it as:

```rust
Error::Http(...)
```

## How `?` uses `From`

The `?` operator is a Rust language feature, not an async feature and not a Tokio feature.

This:

```rust
let response = client.send().await?;
```

means roughly:

```rust
let response = match client.send().await {
    Ok(value) => value,
    Err(error) => return Err(std::convert::From::from(error)),
};
```

For ordinary `Result`, the practical mental model is:

> `?` unwraps `Ok`, and on `Err` it returns early after converting the error with `From`.

In cawir:

```rust
.send().await?
```

starts from:

```rust
Result<reqwest::Response, reqwest::Error>
```

but `ask_claude` returns:

```rust
Result<String>
```

and cawir's alias means:

```rust
std::result::Result<String, Error>
```

So on failure, `?` must turn a `reqwest::Error` into an `Error`.

That works because `#[from]` generated:

```rust
impl From<reqwest::Error> for Error
```

## Manual version without `#[from]`

The compact version:

```rust
#[derive(Debug, Error)]
pub enum Error {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}
```

could be written more explicitly as:

```rust
#[derive(Debug, Error)]
pub enum Error {
    #[error("http error: {0}")]
    Http(reqwest::Error),
}

impl std::convert::From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Error::Http(value)
    }
}
```

Or the call site could avoid automatic conversion and map the error directly:

```rust
let response = client
    .send()
    .await
    .map_err(Error::Http)?;
```

That version says the conversion at the call site:

> If reqwest fails, wrap the error in `Error::Http`.

The `#[from]` version moves that rule into the error type itself.

## Can `#[foo]` replace `#[from]`?

No.

`#[from]` works only because `thiserror` declares that its `Error` derive macro understands that helper attribute.

This is valid:

```rust
Http(#[from] reqwest::Error)
```

This is not:

```rust
Http(#[foo] reqwest::Error)
```

unless some macro specifically declares and understands `foo`.

The IDE often cannot "go to definition" on `#[from]` because it is not an ordinary function or trait. It is macro input consumed during compilation.

## How to know this works

You know this pattern from three sources:

1. The `thiserror` documentation and examples.
2. The Rust trait model: `?` uses `From`-style conversion for ordinary `Result`.
3. Compiler errors. Without a conversion, Rust will complain that it cannot convert `reqwest::Error` into the function's error type.

The syntax is compact, but the underlying idea is simple:

```text
reqwest::Error -> Error::Http(reqwest_error)
```

`thiserror` just writes the repetitive glue.

## `?` with `Option`

The `?` operator also works with `Option<T>`.

With `Result`, the two cases are:

```text
Ok(value)  -> keep going with value
Err(error) -> return Err(error) from the current function
```

With `Option`, the two cases are:

```text
Some(value) -> keep going with value
None        -> return None from the current function
```

This:

```rust
fn first_char(text: &str) -> Option<char> {
    let ch = text.chars().next()?;
    Some(ch)
}
```

means roughly:

```rust
fn first_char(text: &str) -> Option<char> {
    let ch = match text.chars().next() {
        Some(ch) => ch,
        None => return None,
    };

    Some(ch)
}
```

So the enclosing function must return a compatible type. If `?` may need to return `None`, the function cannot promise to return a plain `char`; it needs to return `Option<char>`.

## The takeaway

This line:

```rust
#[error("http error: {0}")]
Http(#[from] reqwest::Error),
```

means:

- Store `reqwest::Error` inside `Error::Http`.
- Print it as `http error: <inner error>`.
- Generate `impl From<reqwest::Error> for Error`.
- Let `?` automatically convert `reqwest::Error` into `Error`.

Compact Rust often hides generated trait implementations. When in doubt, expand it mentally into `match`, `return Err(...)`, and `impl From`.
