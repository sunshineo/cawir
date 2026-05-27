# WebSocket transports and channels

Checkpoint 14e adds WebSocket as a second App Server transport.

## Tokio features are opt-in

The `tokio` crate is split into feature flags. cawir already used Tokio for async
entry points and timers:

```toml
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

WebSocket serving needs more pieces:

```toml
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread", "sync", "time"] }
```

`net` enables `tokio::net::TcpListener`. `sync` enables Tokio's async channels.
Rust crates often expose feature flags so projects do not compile unused modules
and transitive dependencies.

## WebSocket is still just text at the boundary

`tokio-tungstenite` provides async WebSocket support on top of Tokio TCP streams.
The app-server protocol still receives strings:

```rust
Ok(Message::Text(text)) => {
    incoming_sender.send(Ok(text.to_string()))?;
}
```

The JSON-RPC-style parsing happens after that. This keeps WebSocket from leaking
into protocol code. The same protocol loop can process a line read from stdio or a
text frame read from a socket.

## Split streams use extension traits

WebSocket streams implement lower-level `Sink` and `Stream` traits. The convenient
methods come from `futures-util` extension traits:

```rust
use futures_util::{SinkExt, StreamExt};

let (mut writer, mut reader) = websocket.split();
reader.next().await;
writer.send(Message::Text(text.into())).await;
```

The import matters. Without `SinkExt` and `StreamExt`, methods like `split`,
`next`, and `send` are not in scope even though the underlying type supports the
traits.

## Channels bridge async transport and sync approvals

cawir's approval callbacks are currently synchronous:

```rust
FnMut(&ToolApprovalRequest) -> Result<bool>
```

That is why the WebSocket transport uses channels:

```text
WebSocket reader task -> incoming channel -> protocol loop
protocol loop -> outgoing channel -> WebSocket writer task
```

The incoming side uses `std::sync::mpsc::Receiver` because the approval callback
may need to wait for a client response without being async. The outgoing side uses
`tokio::sync::mpsc::UnboundedSender` because sending from synchronous protocol code
to an async writer task does not require `.await`.

This is a pragmatic bridge, not a final async architecture. Waiting for human
approval can block one Tokio worker thread today. That is acceptable for this small
single-client server, but if cawir later becomes a multi-client daemon, the better
design is to make approval callbacks async and remove the blocking receive.

## Async transport is not the same as an async callback

WebSocket I/O is async:

```rust
reader.next().await;
writer.send(...).await;
```

But cawir's approval callback still has a synchronous type:

```rust
FnMut(&ToolApprovalRequest) -> Result<bool>
```

That callback cannot use `.await` directly. The channel bridge converts async
transport messages into a blocking receive that the synchronous callback can use.

An async approval callback would look conceptually more like:

```rust
async fn approve_tool(request: &ToolApprovalRequest) -> Result<bool>;
```

Rust traits and callbacks that return futures need more type machinery than normal
synchronous callbacks. That is worth doing when the architecture needs it, such as
for multi-client daemon behavior, but not just to add the first WebSocket transport.

## Keep the trait at the narrowest useful level

The new transport trait only knows two operations:

```rust
fn read_text(&mut self) -> io::Result<Option<String>>;
fn send(&mut self, message: &ServerMessage) -> io::Result<()>;
```

It does not know about sessions, tools, providers, or JSON-RPC methods. That keeps
the abstraction honest: transports carry bytes or text, while the protocol decides
what those messages mean.
