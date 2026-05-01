# Closures and destructuring

Rust closures are anonymous functions. They are similar to JavaScript/TypeScript arrow functions.

```rust
let add_one = |x| x + 1;
```

is the Rust shape of:

```ts
const addOne = (x) => x + 1;
```

The pipes hold the closure parameters:

```rust
|x| x + 1
```

Multiple parameters go inside the same pipes:

```rust
|a, b| a + b
```

## `map` takes a closure

`map` is a method on an iterator. The closure is the argument passed to `map`.

```rust
items.into_iter().map(|item| item.name).collect()
```

This is similar to:

```ts
items.map((item) => item.name)
```

So:

- `.map(...)` is the method call.
- `|item| item.name` is the closure.

## Closure parameters can be patterns

Rust closure parameters are not limited to simple names. They can be patterns.

This parameter:

```rust
|ToolResult {
    tool_use_id,
    content,
    is_error,
}|
```

means:

> The closure receives one `ToolResult`, and immediately pulls its fields into local variables named `tool_use_id`, `content`, and `is_error`.

This compact version:

```rust
results
    .into_iter()
    .map(|ToolResult {
        tool_use_id,
        content,
        is_error,
    }| MessageContent::ToolResult {
        tool_use_id,
        content,
        is_error,
    })
    .collect()
```

is equivalent to this more explicit version:

```rust
results
    .into_iter()
    .map(|result| {
        let ToolResult {
            tool_use_id,
            content,
            is_error,
        } = result;

        MessageContent::ToolResult {
            tool_use_id,
            content,
            is_error,
        }
    })
    .collect()
```

## Why `into_iter()` matters here

`into_iter()` consumes the vector and gives the closure owned `ToolResult` values.

That means destructuring can move the owned `String` fields out of `ToolResult` and into `MessageContent::ToolResult` without cloning:

```rust
MessageContent::ToolResult {
    tool_use_id,
    content,
    is_error,
}
```

If the code used `.iter()` instead, the closure would receive references to the items, and moving the `String` fields out would not be allowed.

## The takeaway

Read this:

```rust
|ToolResult { tool_use_id, content, is_error }| ...
```

as:

```text
take one ToolResult argument, unpack its fields, then run the closure body
```

The pipes are closure syntax. The `ToolResult { ... }` part is destructuring.

## Passing callable behavior into a function

Checkpoint 3h used a closure-like parameter to make `write_file` testable:

```rust
fn execute_write_file_with_approval<F>(input: &Value, mut approve: F) -> Result<String>
where
    F: FnMut(&str, &str) -> Result<bool>,
{
    if !approve(path, content)? {
        // denied
    }
}
```

This means:

> `approve` can be any callable value that takes `&str` path and `&str` content, and returns `Result<bool>`.

The production path passes a normal function:

```rust
execute_write_file_with_approval(input, approve_write_interactively)
```

Tests pass small closures:

```rust
execute_write_file_with_approval(&input, |_, _| Ok(true))
execute_write_file_with_approval(&input, |_, _| Ok(false))
```

This is dependency injection without introducing a trait object or interface. The write logic does not know whether approval came from stdin, a test closure, or some future policy layer. It only knows it can call `approve(path, content)`.

## The three callable traits

Rust models callable values with three standard traits:

```rust
FnOnce
FnMut
Fn
```

Their relationship is about what the callable is allowed to do with captured state:

- `FnOnce` can be called at least once and may consume captured values.
- `FnMut` can be called repeatedly and may mutate captured state.
- `Fn` can be called repeatedly without mutating captured state.

`FnMut` was a useful middle choice for approval because it accepts ordinary functions, simple test closures, and closures that might update captured test state.

The binding itself must be mutable:

```rust
mut approve: F
```

because calling an `FnMut` may mutate the callable value.
