# Async/sync — Rust's choice vs other languages

A common confusion when coming to Rust from another language: is async/await mandatory, optional, or default?

## The key insight

Rust has **both** sync and async. The choice happens at the **library level**, not the language level. Rust's stdlib I/O (`std::io`, `std::net`) is sync. Async is opt-in via libraries (`tokio` + `reqwest`, `async-std`, etc.).

This makes Rust unlike every mainstream language:

| Language | Default I/O model | Alternative |
|---|---|---|
| **Python** | Sync (`requests`, `urllib`). Always blocks. | `aiohttp` + `asyncio` — opt-in async, separate world |
| **Java** | Sync historically. Threading for concurrency. | `HttpClient.sendAsync`, virtual threads (Project Loom) |
| **Node.js** | **Async by default.** Even file I/O is async. | Sync versions exist but discouraged |
| **Rust** | **No default.** Stdlib is sync; ecosystem is async-first. | You pick per project, by choice of library |

## What sync vs async looks like in Rust

### Sync version of cawir's HTTP call

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("cawir/0.1")
        .build()?;
    let zen = client.get("https://api.github.com/zen").send()?.text()?;
    println!("{}", zen);
    Ok(())
}
```

No `tokio`. No `async fn`. No `.await`. Single HTTP call: same elapsed time as the async version.

### Async version (what we actually wrote at 2a-ii)

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .user_agent("cawir/0.1")
        .build()?;
    let zen = client.get("https://api.github.com/zen").send().await?.text().await?;
    println!("{}", zen);
    Ok(())
}
```

Same logic. Different shape. The async version isn't faster for one call — it's the right shape for handling streams (CP5) and concurrent tool calls (later).

## Why cawir picked async

Could have stayed sync. We chose async because:

1. **Ecosystem trend is async-first.** Most modern Rust HTTP / network code is async; sync is now often a wrapper.
2. **Streaming (CP5) needs async.** You can't `.await` a stream from a sync function.
3. **Future tool-use parallelism wants async.** Multiple tools running concurrently is much cleaner with async tasks than threads.
4. **Sync → async is a bigger refactor later than starting async.**

For a CLI that makes one HTTP call and exits, sync is cleaner. For an agent that streams responses and runs many concurrent operations, async is the right shape.

## Comparison to each language

### Python

```python
# Sync — what you'd write naturally
import requests
zen = requests.get("https://api.github.com/zen").text
```

Async exists but is a separate world:

```python
# Async — requires asyncio
import asyncio, aiohttp
async def main():
    async with aiohttp.ClientSession() as session:
        async with session.get("...") as resp:
            print(await resp.text())
asyncio.run(main())
```

A sync Python function can't call an async one without `asyncio.run()`. The two ecosystems don't compose smoothly.

**Rust similarity:** also two worlds, also a "boundary" (the runtime) between them.
**Rust difference:** Rust's `async fn` returns a regular `Future` value — you can store, compose, transform it. Python's coroutines feel like an afterthought stitched onto a sync language.

### Java

Traditional Java is thread-based:

```java
HttpClient client = HttpClient.newHttpClient();
HttpResponse<String> resp = client.send(req, BodyHandlers.ofString());
```

Threads block. The JVM gives you many threads. Project Loom (Java 21+) introduced virtual threads — sync-looking code that doesn't block OS threads.

**Rust similarity:** sync I/O looks similar.
**Rust difference:** Java threads are heavy (~1 MB stack each). Rust async tasks are nearly free. Rust has no JVM, no garbage collector.

### Node.js — closest match in feel

```javascript
async function main() {
    const resp = await fetch("https://api.github.com/zen");
    const zen = await resp.text();
    console.log(zen);
}
```

Looks **almost identical** to Rust async:

```rust
async fn main() -> Result<(), Box<dyn Error>> {
    let resp = client.get("...").send().await?;
    let zen = resp.text().await?;
    println!("{}", zen);
}
```

Same syntax, same shape, same mental model.

Underneath, quite different:

| Aspect | Node.js | Rust |
|---|---|---|
| Event loop | Built into the runtime, always there | External library (tokio); your choice |
| Threading | Single-threaded by default | Multi-threaded by default (`rt-multi-thread`) |
| "Forgot to await" | Unhandled Promise warning | Future is dropped silently (compiler usually warns via `#[must_use]`) |
| Cost per task | Each Promise allocates | Zero-cost: stackless coroutines |
| Cancellation | Manual plumbing | Drop the Future and it cancels |

### C# / Kotlin — actually the closest match

C#'s `async`/`await` + `Task<T>` is conceptually identical to Rust's `async`/`await` + `Future<T>`. Same model: stackless coroutines, syntax sugar over compiler-generated state machines.

Kotlin coroutines (`suspend` + `await`) are also the same model.

Rust adopted this syntax in 2019, inspired by C# (which pioneered it in 2012).

Differences from C#/Kotlin:

- **No GC** — Rust async tasks have no allocations beyond what your code asks for.
- **No built-in runtime** — C# has `TaskScheduler`; Rust uses external runtimes.
- **Lifetimes** — Rust's borrow checker applies to async too. References can't outlive their data, even across `.await` points.

## When does each model show up?

| If you're writing | Use |
|---|---|
| A CLI that makes 1-N HTTP calls and exits | Sync is cleaner (`reqwest::blocking`) |
| A long-running server | Async (concurrent connections) |
| A streaming consumer (LLM tokens, websockets) | Async (need to consume incrementally) |
| Embedded / `no_std` | Async, but with `embassy` not `tokio` |
| Thousands of concurrent operations | Async (threads don't scale that high) |

cawir is the third row plus parallel tool calls in the future. Async is right.

## Mental-model summary

- **Rust async syntax looks like Node.js / C#.** If you know either, the syntax transfers.
- **Rust's stdlib I/O is sync** like Python or pre-Loom Java. Async is opt-in.
- **Choice of async is a library choice, not a language choice.** You can write fully sync Rust.
- **For one-off operations, sync is cleaner.** For concurrency or streaming, async is right.
- **Underneath the syntax**, Rust async is closer to C#/Kotlin than to Node — stackless coroutines, no allocator overhead, runtime is external.

## See also

- `09-async-tokio-and-runtime.md` — what async is conceptually, why main has to be async, what tokio is
