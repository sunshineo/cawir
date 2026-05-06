# Async trait methods

Checkpoint 4e added async behavior to the `Provider` trait:

```rust
pub(crate) trait Provider {
    async fn available_models(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
    ) -> Result<Vec<String>>;

    async fn send(
        &self,
        client: &reqwest::Client,
        credential: &ActiveCredential,
        model: &str,
        messages: &[Message],
        tools: Vec<ToolDefinition>,
    ) -> Result<ProviderResponse>;
}
```

An `async fn` does not immediately run to completion. It returns a future, and `.await` drives that future until it produces its output.

Inside a trait, this means each implementor returns its own concrete future type. `Anthropic::available_models`, `OpenAi::available_models`, and `Ollama::available_models` can all have different hidden future types, even though callers see the same trait method signature.

## Why this works here

cawir currently uses static dispatch and enum dispatch:

```rust
enum ActiveProvider {
    Anthropic(Anthropic),
    OpenAi(OpenAi),
    Ollama(Ollama),
}
```

The enum implements `Provider` by matching each variant and forwarding to the concrete provider:

```rust
match self {
    Self::Anthropic(provider) => provider.available_models(client, credential).await,
    Self::OpenAi(provider) => provider.available_models(client, credential).await,
    Self::Ollama(provider) => provider.available_models(client, credential).await,
}
```

Each `match` arm calls a concrete provider method, so the compiler can still know the future type in that branch. This keeps the code simple and avoids adding another crate just to box async trait futures.

## Object safety caveat

This would become trickier if cawir changed the active provider to a trait object:

```rust
Box<dyn Provider>
```

Traits with async methods are not object-safe in the same straightforward way as ordinary sync methods. A trait object needs a vtable entry with one stable method shape, but an async method hides an implementor-specific future type.

Common options if cawir later needs `Box<dyn Provider>`:

- keep enum dispatch and accept the forwarding boilerplate
- manually return a boxed future from the trait method
- use a crate such as `async-trait`, which boxes the futures for you

For now, enum dispatch is a good learning shape because the provider set is small and the async control flow remains explicit.
