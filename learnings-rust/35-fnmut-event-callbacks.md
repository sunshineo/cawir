# `FnMut` event callbacks

Checkpoint 8 passes agent progress through this kind of parameter:

```rust
emit: &mut impl FnMut(AgentEvent)
```

Read it as:

```text
give this function a callable event consumer; it can be called many times and may mutate its captured state
```

`FnMut` does not mean the callback mutates the `AgentEvent`. The event is passed into the callback as a value:

```rust
emit(AgentEvent::ModelRequestStart {
    provider: provider.name().to_string(),
    model: model.to_string(),
});
```

The `Mut` part means the callback may mutate state it captured from the surrounding scope.

## Captured state versus local state

This closure is `FnMut` because it mutates `events`, which was created outside the closure:

```rust
let mut events = Vec::new();

let mut emit = |event| {
    events.push(event);
};
```

That is useful in tests because the test can collect emitted events and assert on them later.

This closure can still be `Fn` because `line` is local to each call:

```rust
let emit = |event| {
    let mut line = String::new();
    line.push_str("event: ");
    line.push_str(&format!("{event:?}"));
    println!("{line}");
};
```

`Fn` does not forbid all mutation. It forbids mutating captured state through the closure. Local variables created inside one call are normal mutable locals.

## The three callable traits

Rust closures implement the least-powerful callable trait that fits what they do:

- `Fn` can be called repeatedly and does not mutate captured state.
- `FnMut` can be called repeatedly and may mutate captured state.
- `FnOnce` can be called at least once and may consume captured values.

An event callback needs to be called repeatedly during one agent turn, so `FnOnce` is too weak.

An event consumer often wants to count, buffer, collect, log, or render with internal state, so `Fn` is too strict.

`FnMut` is the practical middle choice:

```rust
let mut event_count = 0;

let mut emit = |event: AgentEvent| {
    event_count += 1;
    render_agent_event(event);
};
```

The callback mutates `event_count`, not the event itself.

## Why `&mut`

Calling an `FnMut` may mutate the callable value, so the caller must pass it mutably:

```rust
agent::run_turn(..., &mut render).await?;
```

This is the same Rust rule as passing `&mut Vec<T>` when a function may push into the vector: the function needs mutable access to the thing it is allowed to change.

## Functions also fit

A plain function like this can be passed where `FnMut` is expected:

```rust
fn render_agent_event(event: AgentEvent) {
    println!("{event:?}");
}
```

It does not mutate captured state because it captures nothing. Since it satisfies the stricter `Fn` behavior, it can also be used where `FnMut` is accepted.

