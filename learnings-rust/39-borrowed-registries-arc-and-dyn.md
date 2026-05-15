# Borrowed registries, `Arc`, and `dyn`

Checkpoint 8.5e moved `ToolRegistry` ownership into `Runtime` and made the agent loop borrow it explicitly.

## Borrowing the registry

The current turn context stores:

```rust
pub(crate) tool_registry: &'a ToolRegistry,
```

`&ToolRegistry` means the agent turn gets a temporary read-only borrow. The REPL still owns `Runtime`, `Runtime` owns the registry, and `run_turn` can use the registry only while that borrow is valid.

This fits the current control flow:

```text
REPL owns Runtime
Runtime owns ToolRegistry
run_turn borrows &ToolRegistry
borrow ends when the turn ends
```

Borrowing is the simplest correct shape when there is one owner and a shorter-lived function only needs access.

## What `Arc<T>` is for

`Arc<T>` means atomically reference-counted `T`.

It gives shared ownership:

```rust
use std::sync::Arc;

let registry = Arc::new(ToolRegistry::builtins());
let first = Arc::clone(&registry);
let second = Arc::clone(&registry);
```

`Arc::clone` does not clone the `ToolRegistry`. It clones the pointer and increments a thread-safe reference count. The registry is freed only after the last `Arc` owner is dropped.

Compared to common pointer shapes:

```text
Box<T> = one owner, heap allocation
&T     = borrowed access, no ownership
Rc<T>  = shared ownership, single-threaded
Arc<T> = shared ownership, thread-safe
```

cawir does not need `Arc<ToolRegistry>` yet because no spawned task or background worker needs to own the registry independently. If hooks, concurrent agent loops, or background tool users later need long-lived access, `Arc<ToolRegistry>` may become the right tool.

## `dyn Tool` and Java interfaces

`dyn Tool` is a trait object. It is close to using an object through a Java interface:

```rust
trait Tool {
    fn name(&self) -> &'static str;
}

struct ReadFileTool;
struct ShellTool;

impl Tool for ReadFileTool { /* ... */ }
impl Tool for ShellTool { /* ... */ }
```

`Box<dyn Tool>` means:

```text
a heap-owned value of some concrete type that implements Tool,
called through the Tool interface at runtime
```

The registry uses:

```rust
Vec<Box<dyn Tool>>
```

because a `Vec` needs one element type, but `ReadFileTool`, `ShellTool`, and `EditFileTool` are different concrete types. The `Box<dyn Tool>` wrapper gives them one common outer type.

Calling a method through `dyn Tool` uses dynamic dispatch. Rust follows a vtable pointer at runtime to call the implementation for the actual concrete tool. This has a small runtime cost, but it is a good fit for registries that hold mixed capability types.
