# Rust error handling — language enforcement vs application policy

A genuine tension: the compiler enforces that you **acknowledge** every fallible operation, but it does NOT enforce that your handling is **good for production**. Understanding the split clarifies a lot of "should I write this differently?" questions.

## What the compiler forces

Every fallible operation in Rust returns `Result<T, E>` (or `Option<T>` for "might be absent"). The compiler **forces you to acknowledge it at the call site**. Your options:

| Syntax | Means | Ergonomic? |
|---|---|---|
| `let x = foo()?;` | "Propagate the error up." | ✅ ergonomic |
| `let x = foo().unwrap();` | "If err, panic." | ⚠️ stigmatized |
| `let x = foo().expect("foo failed");` | Same with a message. | ⚠️ stigmatized |
| `match foo() { Ok(x) => ..., Err(e) => ... }` | Handle both cases. | ✅ explicit |
| `if let Ok(x) = foo() { ... }` | Handle `Ok`, deliberately ignore `Err`. | ✅ explicit |
| `let _ = foo();` | Throw away the result. | ✅ explicit |

You **cannot silently ignore** a `Result`. The `#[must_use]` lint on `Result` is on by default. Even `let _x = foo()` requires the `_` if `foo` returns `Result`.

## What the compiler does NOT force

Whether your choice is *appropriate for the situation*. The compiler accepts:

- `?` propagating all the way to `main` → program exits on first failure
- `unwrap()` everywhere → program panics on first failure
- Explicit `match` with recovery → robust handling

It treats all three as "you addressed the Result." Whether you addressed it well is your call.

## Java comparison — Rust is actually stricter

A common misconception: "Java forces error handling, Rust doesn't." It's the opposite.

| | Compiler enforces handling? | Notes |
|---|---|---|
| Java **checked** exceptions (`IOException`, `SQLException`) | ✅ Must catch or declare in `throws` | Less common in modern Java |
| Java **unchecked** exceptions (`RuntimeException`, `NullPointerException`, most modern stuff) | ❌ Silently propagates and crashes | Most modern Java is here |
| Rust `Result<T, E>` | ✅ Every call site must acknowledge | All errors |
| Rust `panic!` | ❌ Escape hatch — but stigmatized via lints | |

Most modern Java throws unchecked exceptions. Spring, Jackson, JDBC drivers all throw `RuntimeException` subtypes the compiler doesn't force you to handle. A Java program calling `objectMapper.readValue(json, Foo.class)` can crash at runtime with no compile-time warning.

Rust pulls every fallible operation into the type system. **There is no silent crash equivalent in safe Rust** — every error site is forced to be acknowledged.

## The fail-fast vs graceful axis

Independent of language enforcement, application code chooses a policy.

### Fail-fast (`?` everywhere, propagate to main)

```rust
fn main() -> Result<(), Box<dyn Error>> {
    let response = client.get(url).send().await?;       // network error → exit
    let body = response.text().await?;                  // body read error → exit
    let parsed: Foo = serde_json::from_str(&body)?;     // parse error → exit
    process(parsed)?;                                    // logic error → exit
    Ok(())
}
```

**Pros:** concise, errors surface immediately during development, easy to add error context as you go.
**Cons:** any failure is total. User-facing experience is "exit code 1 with a debug-printed error." No retry, no recovery.
**When right:** prototypes, scripts, single-developer CLIs, this-will-only-be-used-once tools.

### Graceful (`match` / `if let` / explicit recovery)

```rust
async fn handle_user_input(input: &str, session: &mut Session) {
    let response = match client.post(url).json(&req).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("network error: {}; please try again", e);
            return;
        }
    };

    let parsed: MessageResponse = match response.json().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("couldn't parse response: {}", e);
            return;
        }
    };

    // ... continue normally
}
```

**Pros:** single failure doesn't end the program. User sees actionable messages. Retry, fallback, logging are possible.
**Cons:** more code per error site. Can feel repetitive. Decision fatigue per call.
**When right:** shipped applications, long-running services, anything where partial success matters.

## Lints that move the needle

Rust ships tools to enforce stricter policies than the language defaults:

| Lint / flag | Effect |
|---|---|
| `#![deny(unused_must_use)]` (on by default) | Can't silently ignore a `Result` |
| `#![deny(clippy::unwrap_used)]` | Forbids `.unwrap()` in your crate |
| `#![deny(clippy::expect_used)]` | Forbids `.expect()` in your crate |
| `#![deny(clippy::panic)]` | Forbids `panic!()` invocations |
| `#![warn(clippy::pedantic)]` | A bag of "you might want to look at this" lints |
| `cargo clippy -- -D warnings` | Turns clippy warnings into hard errors in CI |

These tighten *which handling styles are allowed* but they don't catch *"you used `?` when you should have used `match`"*. That decision stays yours.

## What cawir does, and when it changes

cawir's policy at 2c: **fail-fast everywhere.** Every fallible operation uses `?` and propagates up to `main`. If anything goes wrong, the program exits.

This is correct for the current stage:

- The author is learning Rust; errors during learning should be loud and obvious.
- Code added for graceful handling distracts from the concept being taught.
- Anybody running cawir at 2c is the developer themselves, who can read a stack trace.

It becomes the *wrong* default starting at:

- **2d (Wire with REPL)** — every user input becomes a Claude call. A single bad response should not end the session. The Claude-call site switches from `?` to `match` for per-call recovery; the REPL keeps running.
- **2f (Cleanup)** — `thiserror` enum replaces `Box<dyn Error>`. Different error variants can be handled differently (network vs parse vs API auth).
- **CP4 (Modes) onward** — the agent loop has many failure modes; each gets nuanced handling.

This follows the "grow into" discipline from the architecture doc — no speculation, no over-engineering, complexity added when the situation demands it.

## The takeaway

- **Rust forces handling, doesn't enforce policy.** Java forces less, but also doesn't enforce policy. Both leave "good handling" to the developer.
- **`?` is ergonomic because propagation is often correct.** Don't feel bad using it.
- **Fail-fast is fine for prototypes, wrong for production.** Tightening happens during cleanup, not up front.
- **Lints can enforce stricter policies** when you're ready.
- **Rust's compile-time enforcement is stricter than Java's**, contrary to common assumption.

## See also

- `07-result-question-mark-unit.md` — basics of `Result<T, E>`, `Ok`/`Err`, the `?` operator, the unit type, and how `fn main() -> Result` enables `?`.
