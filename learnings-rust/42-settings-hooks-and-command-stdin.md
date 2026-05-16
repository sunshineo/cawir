# Settings, hooks, and command stdin

Checkpoint 9 added three related Rust patterns: merging JSON settings, storing hook handlers behind trait objects, and running command hooks with structured input/output.

## Open-ended settings with `serde_json::Value`

`serde_json::Value` is useful when cawir wants to preserve an open-ended settings shape:

```rust
fn deep_merge(target: &mut Value, source: Value) {
    match (target, source) {
        (Value::Object(target), Value::Object(source)) => {
            for (key, value) in source {
                match target.get_mut(&key) {
                    Some(existing) => deep_merge(existing, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (target, source) => {
            *target = source;
        }
    }
}
```

Objects merge key-by-key. Scalars and arrays replace the older value. That gives a simple precedence rule: load user settings first, then project settings, then local settings, so local values win.

The important design split is:

```text
SettingsResolver loads and merges open-ended JSON.
HookRegistry parses only the hook section into typed Rust structs.
```

That avoids designing the whole future settings schema before the project has real consumers for every key. The part that drives behavior is still typed before execution.

This is different from session data. `Session` is durable cawir-owned data, so it should stay strongly typed. Settings are an extension surface, so the outer shape can stay flexible while specific consumers parse their own section.

## `Box<dyn HookHandler>`

The hook registry stores handlers like this:

```rust
BTreeMap<HookEventKind, Vec<Box<dyn HookHandler>>>
```

`dyn HookHandler` is a trait object. It means "some concrete type that implements `HookHandler`, chosen at runtime." `Box` puts that concrete handler on the heap so different handler structs can live in the same `Vec`.

Without `dyn`, a vector can hold only one concrete type:

```rust
Vec<CommandHook>
```

That would work for checkpoint 9 because only command hooks exist. But the architecture already names later handler flavors such as prompt hooks and agent hooks. `Vec<Box<dyn HookHandler>>` lets those future handler types sit beside `CommandHook` without changing the registry shape.

The tradeoff is dynamic dispatch: Rust calls `on_event` through a vtable at runtime. That is a small cost here, and it buys the right extension seam at the point where multiple hook implementations are expected.

## `BTreeMap` for deterministic registries

The hook registry stores handlers by event kind:

```rust
BTreeMap<HookEventKind, Vec<Box<dyn HookHandler>>>
```

`BTreeMap` keeps keys ordered. Ordering is not important for a single lookup, but deterministic containers make tests and debug output easier to reason about. A `HashMap` would also work functionally, but its iteration order is intentionally not stable.

## Writing to a child process stdin

`Command::output()` is enough when a process only needs arguments. Hook commands need the event JSON on stdin, so cawir uses `spawn()`:

```rust
let mut child = Command::new("sh")
    .arg("-c")
    .arg(command)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;

let mut stdin = child.stdin.take().ok_or_else(...)?;
stdin.write_all(&event_json)?;
drop(stdin);

let output = child.wait_with_output()?;
```

`take()` moves the stdin pipe out of the child handle. `drop(stdin)` closes the pipe so the hook process sees EOF and can finish reading.

## Tagged stdout actions

Hook stdout is parsed with serde's internally tagged enum pattern:

```rust
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum CommandHookOutput {
    Allow,
    Deny { message: String },
    Modify { input: Value },
}
```

This is the same idea as event serialization, but with a different tag field. JSON like `{"action":"deny","message":"blocked"}` becomes a typed Rust enum instead of a hand-parsed string.
