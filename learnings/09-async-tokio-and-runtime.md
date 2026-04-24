# Async, tokio, and what "runtime" means in Rust

Two related questions came up at 2a-i. What is async *about*, conceptually? And when we add tokio, does that undermine Rust's "compile to a native binary, no install needed" promise?

## 1. What async is about

Async is fundamentally about **handling slow operations without blocking the thread**.

A "slow operation" in computing is almost always I/O:

| Operation | Typical duration |
|---|---|
| Network request | milliseconds to seconds |
| Disk read | milliseconds |
| User input | seconds to forever |
| CPU add | nanoseconds |
| RAM read | tens of nanoseconds |

I/O is **millions of times slower** than CPU work. A purely synchronous program spends most of its time *waiting* for I/O instead of computing.

### The waiter analogy

**Synchronous waiter.** Takes your order, walks to the kitchen, **stands there watching the chef cook**, brings the food back, takes the next table's order. While waiting for the kitchen, does nothing. Serves 5 tables per hour.

**Async waiter.** Takes your order, drops it at the kitchen, takes the next table's order, drops *that* at the kitchen. When a dish is ready, picks it up and delivers. Always doing something useful. Serves 30 tables per hour.

The second waiter isn't *faster* at any individual task — walking to the kitchen takes the same time. What changed is that they don't *block* on the kitchen; they yield at "wait for food" moments and do other work in between.

In Rust code:

```rust
// Synchronous: thread stands there "kitchen-watching"
let response = http_get("url");
process(response);

// Async: thread yields while waiting, comes back when ready
let response = http_get("url").await;
process(response);
```

`.await` is "drop the request at the kitchen and do other work until it's ready." The **runtime** is the restaurant manager who tracks which dishes are done and tells the waiter when to pick them up.

### When async matters

Async helps when you have **multiple concurrent slow operations to overlap**. For:

- One HTTP call at a time → sync vs async is roughly equivalent in elapsed time
- 10,000 concurrent HTTP calls (a server) → async is dramatically better; 10,000 threads would melt the OS, 10,000 async tasks on one thread scale cleanly

Async is *not* inherently faster. It's about **efficient overlap** of concurrent waits.

For cawir today, the async benefits are small. Real payoff arrives at:

- **CP5 (Streaming)** — consuming response chunks as they arrive from the model
- **Future parallel tool use** — running multiple tools concurrently when the model emits several calls

We adopt async now because `reqwest` (the HTTP library we'll use) is async-first, and retrofitting async later would be a bigger refactor than adopting it up front.

## 2. Why main must be async

Two reasons bundled together.

### `.await` only works inside an `async fn`

The compiler rejects `.await` in plain functions:

```rust
fn main() {
    let r = reqwest::get("url").await;  // error: `.await` only allowed inside `async fn`
}
```

So if we want to use `.await` anywhere, at least one function on the call stack must be `async`. Making main async is the simplest approach — then everything called from main can await.

### Something has to *drive* the async code

Subtle and critical: **calling an `async fn` doesn't run it.** An `async fn` returns a `Future` — a value representing "work to be done later." Without something that polls that Future, it sits there doing nothing.

```rust
async fn main() { ... }
// With no runtime: main called, returned a Future, dropped, zero work done
```

That's why Rust refuses to compile a bare `async fn main()`. You need a runtime to bridge sync-world (where the OS calls your main) and async-world (where your code lives).

### `#[tokio::main]` is that bridge

The proc macro rewrites your `async fn main` into a sync `fn main` that sets up a tokio runtime and runs the async body on it. Roughly:

```rust
// What you write:
#[tokio::main]
async fn main() -> io::Result<()> { /* body */ }

// What the macro generates:
fn main() -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async { /* body */ })
}
```

You could write the bottom form by hand. The macro just saves typing.

## 3. The word "runtime" is overloaded

Three different things get called "runtime." Rust's relationship to each is different.

| Meaning | What it is | Rust? |
|---|---|---|
| **1. Language runtime / VM** | A separate program on the target machine that executes your code. JVM, CPython, Node.js. | ❌ Not needed. Rust compiles to native machine code. |
| **2. Managed runtime** | Services built into every program, always active. GC, green-thread scheduler, class loader. Go, Java. | ~negligible. A few KB of "set up stack, unwind panics" compiled *into* your binary. No GC, no scheduler. |
| **3. Async runtime** | A library coordinating async tasks. Schedules futures, polls I/O. | ❌ Not built in (deliberate). Tokio is this. |

Adding tokio affects only meaning 3.

## 4. Tokio doesn't break the native-binary story

Tokio is a **Rust library**. Ordinary code, no magic.

When you add `tokio = "1"` to Cargo.toml:

- Cargo downloads tokio's source into `~/.cargo/registry/`
- `cargo build` compiles tokio the same way it compiles your code
- The linker stitches your code + tokio + stdlib + all other deps into **one native executable**
- The result runs on any compatible machine with zero installs

What grows:

- Binary size — ~430 KB → ~1.5 MB after tokio + transitive deps
- Memory at runtime — tokio's task queue, thread pool, OS handles
- Startup time — a few hundred microseconds to init the runtime

What does NOT change:

- `cargo build` — same command, same workflow
- Output format — still a native executable
- Edition handling — tokio's edition and ours coexist; the linker links compiled machine code
- Target platform — still whatever triple we built for

## 5. Tokio doesn't run "behind your back"

This is where async runtime (meaning 3) differs sharply from managed runtime (meaning 2).

**Go's GC.** Always runs. Scans your heap, pauses your code briefly, whether you asked or not. You can't compile Go without it.

**Tokio.** Only acts when your code asks. If your `#[tokio::main] async fn main()` body never `.await`s anything, tokio:

1. Initializes its data structures
2. Enters `block_on` with your future
3. Your future completes immediately (no awaits)
4. Runtime drops, threads clean up
5. Program exits

All within microseconds. No surprise GC pauses, no hidden scheduler, no background threads doing unknown work. Tokio is a library your code explicitly controls, not infrastructure that runs regardless.

## 6. Editions are unaffected

Worth reinforcing because it feels like it might matter:

- tokio declares its own `edition` in its Cargo.toml (currently 2021)
- cawir declares `edition = "2024"`
- rustc compiles each crate under its own edition's rules
- The resulting `.rlib` files are edition-neutral compiled machine code
- The linker doesn't know or care about editions

Adding tokio does nothing unusual to the edition model. See `04-rust-editions.md`.

## One-line takeaways

- **Async is about non-blocking I/O.** Don't block the thread during slow operations; yield at `.await` points.
- **Tokio is a library**, compiled into your binary. Not an external VM, not a managed runtime. Your binary stays self-contained.
- **The word "runtime" is overloaded.** Rust lacks meanings 1 and 2; tokio provides meaning 3.
- **`#[tokio::main]` is just sugar** for creating a tokio runtime and running your async code on it.
- **Calling an `async fn` doesn't run it.** Only the runtime's `block_on` (or equivalent) drives the future to completion.
