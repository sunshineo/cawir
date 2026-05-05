# Enum dispatch, trait objects, and provider selection

Checkpoint 4a introduced a `Provider` trait and multiple concrete provider structs:

```rust
struct Anthropic;
struct OpenAi;
```

Each concrete provider implements the same trait:

```rust
impl Provider for Anthropic { ... }
impl Provider for OpenAi { ... }
```

The REPL then needs one runtime value meaning "the provider selected for this run." Rust has two common ways to model that.

## Option 1: enum dispatch

```rust
enum ActiveProvider {
    Anthropic(Anthropic),
    OpenAi(OpenAi),
}
```

Read `Anthropic(Anthropic)` as:

- first `Anthropic`: the enum variant name
- second `Anthropic`: the concrete struct type stored inside that variant

The repeated name is legal but visually confusing. This would mean the same thing with clearer names:

```rust
enum ActiveProvider {
    AnthropicProvider(Anthropic),
    OpenAiProvider(OpenAi),
}
```

An enum is one concrete type with a known set of variants. A function can return it normally:

```rust
fn active_provider() -> Result<ActiveProvider>
```

The enum can implement `Provider` by forwarding each method to the inner provider:

```rust
impl Provider for ActiveProvider {
    fn name(&self) -> &'static str {
        match self {
            Self::Anthropic(provider) => provider.name(),
            Self::OpenAi(provider) => provider.name(),
        }
    }
}
```

This is a runtime branch over a compile-time-known set of cases. The program checks the enum tag at runtime, but each branch calls a statically known concrete method such as `Anthropic::name` or `OpenAi::name`.

Tradeoffs:

- Pro: concrete and explicit
- Pro: compiler knows every variant
- Pro: no heap allocation or vtable dispatch
- Pro: exhaustive `match` catches missing providers when variants are added
- Con: forwarding boilerplate grows as `providers * trait methods`

## Option 2: trait object

Rust traits are conceptually similar to Java interfaces, but a function cannot return a bare trait by value:

```rust
fn active_provider() -> Provider // invalid
```

Rust needs a concrete return type with known size and layout. A trait object gives that concrete outer type:

```rust
fn active_provider() -> Result<Box<dyn Provider>>
```

`Box<dyn Provider>` means "a heap-allocated value of some concrete type that implements `Provider`." It stores:

- a data pointer to the concrete provider value
- a vtable pointer containing the method implementations for that concrete type

Trait checking is still compile-time. `Box::new(NotAProvider)` fails to compile if `NotAProvider` does not implement `Provider`.

What moves to runtime is method dispatch:

```rust
provider.name()
```

With `Box<dyn Provider>`, Rust follows the vtable pointer to find the correct `name` implementation. This is dynamic dispatch.

Tradeoffs:

- Pro: no manual enum forwarding
- Pro: open-ended; call sites do not need to know every provider variant
- Con: heap allocation with `Box`
- Con: vtable dispatch
- Con: async trait methods and object safety can add extra machinery

## Why cawir starts with enum dispatch

For cawir, the enum version is preferred for now because the provider set is small and explicit:

```text
Anthropic
OpenAI
Ollama later
```

The boilerplate is annoying, but useful while learning because the dispatch is visible. If the provider list or trait method count grows enough that forwarding obscures the code, evaluate a crate that generates enum dispatch.

## Existing crates

This pattern is common enough that crates exist:

- `enum_dispatch`: generates enum-based trait forwarding and aims to replace `Box<dyn Trait>` with static enum dispatch.
- `enum_delegate`: similar goal; implements a trait for an enum whose variants contain types that already implement the trait.

These crates generate the same kind of `match` forwarding code we would write manually.

Do not add one preemptively. First check whether the `Provider` trait has grown too broad; splitting a large trait may be better than hiding a large forwarding implementation behind a macro.
