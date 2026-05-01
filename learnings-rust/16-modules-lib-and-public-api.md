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

## `pub(crate)` for internal module seams

Checkpoint 3.5a moved the concrete Anthropic API code to `src/anthropic.rs`.

Some names need to cross module boundaries inside cawir, but they are not part of the public API that outside crates should use:

```rust
pub(crate) enum ClaudeResponse {
    Text(String),
    ToolUse(Vec<MessageContent>),
}
```

`pub(crate)` means:

> Visible anywhere inside this crate, but not visible to code outside this crate.

That fits an application module seam. `lib.rs` needs to call `anthropic::ask_claude`, and the agent loop needs to match on `ClaudeResponse`, but external callers do not need a stable Anthropic API surface.

This is narrower than `pub`:

```rust
pub(crate) async fn ask_claude(...) -> Result<ClaudeResponse>
```

The function is shared internally, but cawir is not promising it as a library API.

3.5a also kept tests beside the private API structs:

```rust
#[cfg(test)]
mod tests {
    use super::*;
}
```

A child `tests` module can access private items from its parent module through `use super::*`. That is why `MessageRequest` and `MessageResponse` can stay private while still being directly tested.

## Crates, modules, and visibility levels

A Rust crate is the compilation unit. In cawir, `src/lib.rs` is the root of the library crate, and `src/main.rs` is the root of the binary crate.

Inside a crate, code is organized into a module tree:

```rust
mod anthropic;
mod error;
mod session;
```

`mod anthropic;` tells Rust to include `src/anthropic.rs` as a module named `anthropic`. A file on disk is not compiled just because it exists; it must be included by the module tree.

Modules are similar to Java packages or namespaces, but Rust uses modules as privacy boundaries too.

Common visibility levels:

```rust
fn helper() {}
```

Private. Visible to the current module and child modules.

```rust
pub fn public_api() {}
```

Public, as long as the parent modules are also public.

```rust
pub(crate) fn internal_api() {}
```

Visible anywhere inside the current crate, but not visible to external crates. This is useful for internal application seams.

```rust
pub(super) fn parent_only() {}
```

Visible to the parent module.

```rust
pub(in crate::some_module) fn limited() {}
```

Visible only within a specific module path.

Visibility applies separately to structs and their fields:

```rust
pub(crate) struct ToolDefinition {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: serde_json::Value,
}
```

Here, both the struct and its fields are visible throughout the crate.

If the fields were private:

```rust
pub(crate) struct ToolDefinition {
    name: String,
}
```

then other modules could name `ToolDefinition`, but they could not construct it with a struct literal or read `name` directly.

One subtle rule: public visibility is capped by the parent module.

```rust
mod anthropic {
    pub fn ask_claude() {}
}
```

`ask_claude` is marked `pub`, but `anthropic` itself is private. External crates still cannot call `cawir::anthropic::ask_claude`.

## Java comparison

Roughly:

| Java | Rust |
|---|---|
| package | module |
| fully qualified class name | module path |
| `import com.example.Foo` | `use crate::foo::Foo` |
| `public` | `pub` |
| package-private | closest common Rust analogue is `pub(crate)`, but `pub(crate)` is crate-wide |
| `private` | private by default |

The comparison is useful, but not exact.

Java package-private means "visible to classes in the same package." Rust's `pub(crate)` means "visible anywhere inside this crate." Rust can be more precise with `pub(super)` and `pub(in path)`.

For cawir:

```rust
mod anthropic;
```

means Anthropic is an internal module.

```rust
pub(crate) async fn ask_claude(...)
```

means other cawir modules may call it, but external crates should not treat it as public API.

## Module-level functions before traits

Checkpoint 3.5b moved the concrete tools into `src/tools.rs`.

The module exposes two functions to the rest of the crate:

```rust
pub(crate) fn definitions() -> Vec<ToolDefinition>
pub(crate) fn execute_tool_uses(blocks: &[MessageContent]) -> Vec<ToolResult>
```

The helper functions remain private:

```rust
fn execute_tool_call(...)
fn execute_read_file(...)
fn execute_write_file(...)
fn execute_shell(...)
```

This creates a clear module boundary without introducing a trait yet. Other modules can ask:

- What tools are available?
- Execute these tool-use blocks.

They cannot reach into the module and call every helper directly.

This is an idiomatic intermediate shape in Rust: use a module with a small public surface first, then extract traits later when multiple implementations create real pressure. A trait is not required just to organize code by responsibility.
