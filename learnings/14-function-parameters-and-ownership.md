# Function parameters — when to use `&T`, `&mut T`, or `T`

A common stumbling block coming from Python, Java, or JavaScript: in those languages, every object parameter is implicitly a reference. Rust makes the choice explicit, with three distinct forms — and the default is the opposite of what you might expect.

## The three forms

| Param type | Means | Caller after the call |
|---|---|---|
| `x: T` (no `&`) | "I take ownership." Function receives the value; can keep, destroy, or return it. | Caller no longer has it (moved). |
| `x: &T` (shared borrow) | "I just want to look at it." Read-only access. | Caller still owns it; keeps using. |
| `x: &mut T` (exclusive borrow) | "I want to modify it through this borrow." | Caller still owns it (gets it back when borrow ends), but can't use it during the call. |

The default — **no `&`** — means **you take ownership**. `&` is what you write when you do NOT want ownership transfer.

## The default is flipped vs GC'd languages

In Python, Java, JavaScript: every object parameter is implicitly a reference.

```python
def ask_claude(client, api_key, prompt):
    return client.post(...)  # client is a reference; caller still has it
```

You can call this in a loop without thinking. The language made the ergonomic choice for you: everything is shared. The cost is hidden — mutations can propagate through aliased references, and you depend on a garbage collector for cleanup.

In Rust, the same function written without `&` would *consume*:

```rust
async fn ask_claude(client: reqwest::Client, ...) {
    // client is moved in; caller no longer has it after this call
}
```

Calling this in a loop fails on the second iteration:

```
error[E0382]: use of moved value: `client`
   --> src/main.rs:NN:34
    |
NN  |       match ask_claude(client, ...).await { ... }
    |                        ^^^^^^ value used here after move
```

The fix is `&`:

```rust
async fn ask_claude(client: &reqwest::Client, ...) -> ...
```

Now the function borrows `client` and the loop keeps owning it.

**`fn foo(x: &str)` in Rust corresponds to what `fn foo(x)` does in Python.** The `&` is what makes Rust's parameter behave like the implicit reference in a GC'd language.

## Decision guide

When designing a function signature, ask:

| If the function... | Use |
|---|---|
| Just reads the value (no mutation, no keeping) | `&T` |
| Modifies the value but the caller still owns it after | `&mut T` |
| Needs to take ownership (store, transfer, destroy) | `T` |
| Takes a `Copy` type (`i32`, `bool`, `usize`, etc.) | `T` (passing by value is free) |

For non-`Copy` types like `String`, `Vec<T>`, `HashMap<K, V>`, `reqwest::Client` — **default to `&T`** unless you have a specific reason to take ownership.

## Worked example: cawir's `ask_claude`

```rust
async fn ask_claude(
    client: &reqwest::Client,
    api_key: &str,
    prompt: &str,
) -> Result<String, Box<dyn std::error::Error>>
```

All three parameters are borrowed. Why each:

- **`client: &reqwest::Client`** — built once outside the REPL loop with `Client::builder().build()`. Reused for every Claude call. Borrowing means the loop keeps owning it across iterations.
- **`api_key: &str`** — read once from `std::env::var` into a `String`. Reused on every call. We pass `&api_key` and Rust auto-coerces to `&str`.
- **`prompt: &str`** — comes from `line.trim()` which already produces a `&str`. We just read it to build the request body. We never need to own it.

If we'd taken these by value, the loop's second iteration would fail with "use of moved value." The fix would be `.clone()` everywhere — wasteful (extra allocations on every call) — or use `&` from the start (free, correct).

## `&str` vs `&String` — prefer the slice form

Closely related convention: in function parameters, prefer `&str` over `&String`.

- `String` — owned, growable, heap-allocated UTF-8 buffer.
- `&String` — a reference to a `String`.
- `&str` — a "string slice" — a borrowed view into UTF-8 bytes anywhere (a `String`'s heap buffer, a string literal embedded in the binary, a substring of either).

`&str` is more general. It accepts both `&String` (via Rust's deref coercion) and string literals.

```rust
fn show(s: &str) { println!("{}", s); }

let owned: String = "hi".to_string();
show(&owned);        // works — &String coerces to &str
show("literal");     // works — string literals are &'static str
```

`fn show(s: &String)` would only accept `&String`. Less flexible.

Same pattern applies to other "owned vs slice" pairs in the stdlib:

| Owned | Borrowed slice |
|---|---|
| `String` | `&str` |
| `Vec<T>` | `&[T]` |
| `PathBuf` | `&Path` |
| `OsString` | `&OsStr` |

For parameters: take the slice form. Functions that accept a slice work for both owned and borrowed inputs at the call site.

## When `T` (no `&`) is right

Sometimes ownership transfer is exactly what you want:

```rust
// 1. The function needs to keep the value
fn store(name: String) -> Container {
    Container { name }    // name lives inside Container now
}

// 2. The function destroys/consumes the value
fn drain(v: Vec<i32>) -> i32 {
    v.into_iter().sum()   // v is consumed by into_iter()
}

// 3. The value is Copy — passing by value is free
fn double(x: u32) -> u32 {
    x * 2                 // u32 is Copy; no allocation
}

// 4. Builder pattern returning a modified version
fn add_header(req: Request, name: &str, value: &str) -> Request {
    req.header(name, value)   // takes ownership, returns a modified Request
}
```

If your function fits one of these patterns, take ownership. Otherwise default to `&`.

## Mutation is a separate axis

The choice between `T`, `&T`, `&mut T` layers two concerns:

| Concern | Question |
|---|---|
| Ownership | Should the caller still own the value after the call? |
| Mutation | Will the function modify the value? |

Combined:

| Mutation? | Ownership transferred? | Form |
|---|---|---|
| No | No (caller keeps) | `&T` |
| Yes | No (caller keeps) | `&mut T` |
| Either | Yes (caller gives up) | `T` |

So "does this function mutate?" is one axis. "Does the caller keep it?" is the other. Most parameters are read-only AND the caller wants to keep them — that's why `&T` is so common.

## Common mistakes for newcomers from GC languages

1. **Writing `T` thinking it means "reference"** — then getting "use of moved value" errors. The fix: change to `&T`.
2. **Writing `&String` instead of `&str`** — works, but accepts fewer call sites. Less flexible API.
3. **Writing `&mut T` when only reading** — overconstrains; restricts how callers can use the function (can't have other shared borrows alive at the same time).
4. **Writing `T` for every parameter and `.clone()`-ing everywhere** — works but wasteful. Each `.clone()` on a `String` allocates.

The fix for all four is the same heuristic: **default to `&T`** for non-`Copy` types unless you have a specific reason to do otherwise.

## See also

- `learnings/05-ownership-and-borrowing.md` — the foundational rules: `mut`, `&`, `&mut`, and the "many readers OR one writer" principle.
