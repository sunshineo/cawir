# Trait-object registries

Checkpoint 6 introduced registries for tools and slash commands.

The core Rust shape is:

```rust
trait Tool {
    fn name(&self) -> &'static str;
    fn execute(&self, input: &Value, mode: PermissionMode) -> Result<ToolOutput>;
}

struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}
```

`Box<dyn Tool>` is a trait object. Read it as:

```text
a heap-owned value of some concrete type that implements Tool
```

The registry can hold different concrete structs in one vector:

```rust
vec![
    Box::new(ReadFileTool),
    Box::new(ListFilesTool),
    Box::new(ShellTool),
]
```

Without `dyn Tool`, a `Vec` needs every element to have the same concrete type. `ReadFileTool` and `ShellTool` are different types, even if both implement the same trait. The trait object gives the vector one common outer type.

`Vec<Tool>` is not valid because `Tool` is a trait, not a concrete type with a known size and layout. `Box<dyn Tool>` is concrete: the box stores a pointer to the tool value plus a pointer to a vtable, which is Rust's runtime method table for that concrete implementation.

## Why not enum matching

Enum matching would still work:

```rust
enum BuiltinTool {
    ReadFile(ReadFileTool),
    ListFiles(ListFilesTool),
    Shell(ShellTool),
}
```

But enum matching keeps dispatch centralized. The caller has to know every variant and write a `match` that says what to do for each one.

A registry wants a different shape:

```text
find the registered tool named read_file
call its shared Tool interface
```

That is why trait objects fit the checkpoint 6 refactor. Each tool owns its own metadata and execution, and the registry only knows how to look up a name and call the common interface.

Use enum dispatch when the caller benefits from knowing every case. Use trait-object registries when the caller mainly needs named lookup plus a shared interface.

## Object safety

Object safety is not about concurrency. It is about whether Rust can call trait methods through `dyn Trait`.

Not every trait can become `dyn Trait`. A trait object needs methods Rust can call through a vtable at runtime. The methods in `Tool` are object-safe because they do not return `Self`, do not have generic type parameters, and take `&self`.

This kind of method is not object-safe:

```rust
trait Bad {
    fn clone_self(&self) -> Self;
}
```

With `dyn Bad`, Rust does not know what concrete `Self` should be at the call site.

The slash-command registry has one extra wrinkle: commands can call async functions such as provider switching and model listing. An `async fn` in a trait method is not object-safe for `Box<dyn Command>`, so cawir uses an explicit boxed future:

```rust
type CommandFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CommandOutcome, String>> + 'a>>;
```

This is the longhand version of "return some async work tied to the borrowed command/context lifetime." `Pin<Box<...>>` gives the future a stable heap location, which Rust async state machines need once they may contain self-references across `.await`.

The async issue is not "running concurrently." It is that each `async fn` implementation returns a different compiler-generated future type. A trait object vtable needs one return type for the method, so cawir erases those different future types behind `Pin<Box<dyn Future<...>>>`.

## Why this is not used for providers yet

cawir still uses enum dispatch for `ActiveProvider` because the provider set is small and explicit. The tool and command registries are different: they are lists of named capabilities where lookup by name is the main operation.

Use enum dispatch when the caller benefits from knowing all cases. Use a trait-object registry when the caller mainly needs "find the thing named X and run its common interface."
