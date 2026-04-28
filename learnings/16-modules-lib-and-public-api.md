# Modules, `lib.rs`, and the public API

Checkpoint 2f split cawir from one large `main.rs` into a small binary entry point plus library modules:

```text
src/main.rs
src/lib.rs
src/error.rs
src/session.rs
```

This is the first real use of Rust's module system in the project.

## Binary crate and library crate

A Cargo package can contain both:

| File | Crate kind | Purpose |
|---|---|---|
| `src/main.rs` | binary crate | Builds the executable users run |
| `src/lib.rs` | library crate | Holds reusable application code |

After 2f, `main.rs` is intentionally tiny:

```rust
#[tokio::main]
async fn main() -> cawir::Result<()> {
    cawir::run().await
}
```

The package is named `cawir`, so the binary crate can refer to the library crate as `cawir`.

That means:

```rust
cawir::run()
```

means:

> Call the public `run` function exported by the library crate.

And:

```rust
cawir::Result<()>
```

means:

> Use the public `Result` type exported by the library crate.

## `mod` loads another source file

In `src/lib.rs`:

```rust
pub mod error;
pub mod session;
```

These lines tell Rust to compile:

```text
src/error.rs
src/session.rs
```

as modules inside the crate.

Without these `mod` declarations, those files might exist on disk, but Rust would not automatically include them. Rust's module tree is declared from code; files are not discovered just because they exist.

## `pub` controls visibility

This:

```rust
pub mod error;
```

means:

> The `error` module is visible outside this crate.

This:

```rust
pub struct Message {
    pub role: String,
    pub content: String,
}
```

means:

> Code outside `session.rs` can name `Message`, and can directly read/write its `role` and `content` fields.

If the fields were not `pub`, this would fail outside `session.rs`:

```rust
Message {
    role: "user".to_string(),
    content: text.to_string(),
}
```

Rust privacy is module-based. A public struct can still have private fields unless each field is also marked `pub`.

## `pub use` re-exports a name

In `src/lib.rs`:

```rust
pub use error::{Error, Result};
```

This is a re-export.

`Error` and `Result` are defined in `error.rs`, but this line makes them available directly as:

```rust
cawir::Error
cawir::Result
```

instead of requiring callers to write:

```rust
cawir::error::Error
cawir::error::Result
```

This keeps the public API clean while allowing the internal code to stay organized by file.

## Type alias: project-specific `Result`

In `error.rs`:

```rust
pub type Result<T> = std::result::Result<T, Error>;
```

This creates a shorthand.

So this:

```rust
pub async fn ask_claude(...) -> Result<String>
```

means:

```rust
pub async fn ask_claude(...) -> std::result::Result<String, Error>
```

The error type is still present. It is just supplied by the alias.

This is common in Rust applications: define one application error type, then define one application `Result<T>` alias that always uses it.

## Why `Message` moved to `session.rs`

`Message` is conversation data:

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    pub content: String,
}
```

It is not just a local variable inside the Anthropic HTTP call. It represents session history, and later CP9 will persist session history to disk.

Moving it into `session.rs` is a small version of the target architecture's rule:

> Session is pure data.

The type still happens to match Anthropic's message shape today, but its conceptual home is the conversation session.

## The takeaway

- `src/main.rs` builds the executable.
- `src/lib.rs` builds the library crate.
- `mod foo;` includes `foo.rs` in the module tree.
- `pub` exposes modules, types, functions, and fields.
- `pub use` re-exports a name from one module at another path.
- `type Result<T> = ...` is a shorthand, not a new runtime type.

The 2f split did not change behavior; it changed where responsibilities live.

