# Futures streams and SSE

Checkpoint 10 added the first real streaming path. The Rust lesson is that an async HTTP response can be treated as a sequence of byte chunks instead of one final body.

## `bytes_stream()` and `StreamExt`

`reqwest::Response::bytes_stream()` returns a stream. A stream is the async version of an iterator: it yields values over time, and each `next()` needs `.await`.

```rust
let mut stream = response.bytes_stream();

while let Some(chunk) = stream.next().await {
    buffer.extend_from_slice(&chunk?);
}
```

`next()` comes from the `futures-util::StreamExt` extension trait. That is why the crate was added and imported:

```rust
use futures_util::StreamExt;
```

This is similar to calling methods from a trait you imported earlier in Rust. The concrete stream type does not define `next()` directly; the trait adds it.

## SSE is text framed over bytes

Server-sent events are not JSON by themselves. They are text frames, usually separated by a blank line:

```text
event: content_block_delta
data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"hel"}}

```

cawir reads arbitrary byte chunks, buffers them until it sees an SSE frame delimiter, then extracts only the `data:` lines. The provider-specific parser handles the JSON inside `data:`.

The buffering matters because network chunks are not semantic units. One chunk might contain half an event, three events, or a split in the middle of a JSON string. The code should only parse after finding an event delimiter.

## Borrowed request objects

Adding streaming also added one more provider-call input: an event sink callback. Passing every value as a separate argument made `Provider::send` too wide, so cawir now uses a parameter object:

```rust
pub(crate) struct ProviderRequest<'a> {
    pub(crate) client: &'a reqwest::Client,
    pub(crate) credential: &'a ActiveCredential,
    pub(crate) model: &'a str,
    pub(crate) prompt: &'a SystemPrompt,
    pub(crate) messages: &'a [Message],
    pub(crate) tools: Vec<ToolDefinition>,
    pub(crate) events: &'a mut dyn FnMut(ProviderEvent),
}
```

The lifetime `'a` says all borrowed fields must stay valid for the request. This avoids cloning the HTTP client, credential, prompt, or message history just to pass them through the provider boundary.

`tools` is owned because each provider converts tool definitions into its own wire format. Moving the vector into `ProviderRequest` lets the provider consume it without cloning.

## `FnMut` for event sinks

The event sink is:

```rust
&mut dyn FnMut(ProviderEvent)
```

`FnMut` means the callback may mutate captured state. That is useful for both production and tests:

```rust
let mut events = Vec::new();
let mut sink = |event| events.push(event);
```

The closure mutates `events`, so `Fn` would be too restrictive. `FnOnce` would allow only one call, but a stream emits many deltas.
