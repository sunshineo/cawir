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

## Why not a stream yet

Checkpoint 8.5g kept the event boundary as a callback instead of changing it to an async stream.

That is an intentional Rust design choice. A callback has a very small contract:

```rust
emit(event);
```

The event is handled immediately by the caller-provided consumer. The terminal REPL can render it, and tests can push it into a `Vec`.

An async stream would introduce more design questions:

- who owns the stream producer?
- is there buffering?
- what happens if the consumer is slow?
- how does shutdown work?
- do event payloads need to be `Send`?
- can multiple consumers subscribe?

Those questions are real, but cawir only has one surface today: the REPL. `FnMut(AgentEvent)` is enough until a second surface, hook runner, or daemon mode creates pressure for a stream abstraction.

## Multiple callbacks for a shared surface runtime

Checkpoint 14b kept the same callback idea but widened it from "render events" to
"let the surface supply decisions."

The shared runtime turn runner receives callbacks for:

- emitting `AgentEvent` values
- approving tool use
- approving or denying a plan

That lets `runtime.rs` own the common turn loop while `repl.rs` decides how those
interactions look in a terminal. A future App Server can pass callbacks that write
JSON notifications and wait for protocol responses instead.

The Rust idea is the same: `FnMut` is a small interface for "call this repeatedly,
and let it mutate the state it captured." The callback may render to stdout, push
events into a vector, write JSONL, or record a pending approval request. The runtime
does not need to know which surface supplied it.

## Interior mutability inside protocol callbacks

Checkpoint 14b's App Server callbacks need to share the same input/output handles:

- event callback writes JSONL notifications
- tool approval callback writes an approval request and reads the client's response
- plan approval callback does the same for plans

Naively, three closures all want `&mut writer`, which Rust rejects because it would
create multiple mutable borrows at the same time.

The App Server uses `RefCell` and `Cell` for this local, single-threaded callback
coordination:

```rust
let writer_cell = RefCell::new(writer);
let next_request_id = Cell::new(self.next_server_request_id);
```

`RefCell<T>` moves the borrow check from compile time to runtime. Each callback
borrows the writer only while it is writing a line, then releases it. If two mutable
borrows overlapped, `RefCell` would panic, so this is only appropriate when the code
shape makes overlap impossible or easy to reason about.

`Cell<u64>` is for small `Copy` values. It lets callbacks increment the server-side
request counter without needing a mutable borrow of the whole App Server struct.

This is a pragmatic bridge for synchronous callbacks. If App Server later becomes
fully concurrent, this local `RefCell` shape should be revisited in favor of an
explicit async transport/task boundary.
