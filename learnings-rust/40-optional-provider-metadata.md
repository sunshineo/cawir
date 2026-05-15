# Optional provider metadata

Checkpoint 8.5f added provider response metadata without making every provider field mandatory.

The useful Rust pattern is a small owned struct with `Option<T>` fields:

```rust
pub(crate) struct TokenUsage {
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) cache_creation_input_tokens: Option<u64>,
    pub(crate) cache_read_input_tokens: Option<u64>,
}
```

Each provider reports a different shape. Anthropic has `input_tokens`, `output_tokens`, and cache counts. OpenAI chat completions uses `prompt_tokens` and `completion_tokens`. Ollama reports `prompt_eval_count` and `eval_count`.

`Option<u64>` says "this value may not exist" in the type system. That is better than using `0` as a sentinel, because `0` can be a real value: a provider can explicitly report zero cache reads.

The conversion happens near the provider-specific serde structs, then the rest of the agent sees one neutral `ProviderMetadata` shape. That keeps wire-format details local while still making diagnostics available to the core loop and REPL.

