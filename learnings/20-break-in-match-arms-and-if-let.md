# `break` inside `match`, and `if let` for one-case handling

Checkpoint 3c surfaced two related Rust control-flow ideas:

1. `break` inside a `match` arm still breaks the surrounding loop.
2. `if let` is the lightweight way to handle one pattern without writing a full `match`.

## `match` is an expression, not a loop

In cawir's REPL, we have a `loop` and then a `match` inside it:

```rust
loop {
    match ask_claude(...).await {
        Ok(ClaudeResponse::ToolUse(blocks)) => {
            // ...
            break;
        }
        // ...
    }
}
```

The important point is:

> `match` does not create its own looping scope.

So `break` does **not** mean "leave the match arm." It means:

> break the nearest enclosing loop.

That is why `break` inside this arm exits the REPL loop.

This can feel surprising if you mentally treat `match` like a self-contained control-flow block. In Rust, it is closer to a switch-like expression whose arms can still use `return`, `break`, or `continue` to target the surrounding function or loop.

## Mental model: what `break` targets

Rust control-flow keywords target enclosing constructs:

- `return` targets the enclosing function
- `break` targets the enclosing loop
- `continue` targets the enclosing loop

They can appear inside nested expressions as long as there really is an enclosing target.

So this is valid:

```rust
loop {
    match value {
        0 => break,
        _ => {}
    }
}
```

But this is not:

```rust
match value {
    0 => break,
    _ => {}
}
```

There is no enclosing loop, so `break` has nowhere to go.

## `if let` handles one interesting pattern

In 3c we changed:

```rust
match handle_tool_use_response(&blocks) {
    Ok(()) => {}
    Err(e) => eprintln!("error: {}", e),
}
```

to:

```rust
if let Err(e) = handle_tool_use_response(&blocks) {
    eprintln!("error: {}", e);
}
```

This reads as:

> If the value matches `Err(e)`, run this block. Otherwise do nothing.

That is the main use of `if let`: you care about one pattern, and the other cases do not need their own code.

## `if let` vs `match`

Use `if let` when:

- one case matters
- the other cases are "ignore" or "do nothing"
- the shorter form is clearer

Use `match` when:

- multiple cases need code
- you want exhaustiveness to be obvious
- the branches are logically important to compare side by side

So:

```rust
if let Err(e) = result {
    eprintln!("error: {}", e);
}
```

is usually better than:

```rust
match result {
    Err(e) => eprintln!("error: {}", e),
    Ok(_) => {}
}
```

## Why this mattered in 3c

3c intentionally stops the REPL after printing a tool result, because the turn is incomplete until 3d sends a `tool_result` back to Claude.

That made the control flow important:

- handle any local error from tool execution
- clean up the incomplete turn in history
- `break` the REPL loop honestly

The `if let Err(e) = ...` form kept the "only the error case matters" part concise, and the `break` inside the `match` arm correctly exited the surrounding REPL loop.
