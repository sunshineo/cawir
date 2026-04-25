# Serde deserialization — fields, null, and async reads

Specific behaviors of `serde::Deserialize` that surprise newcomers. Examples use JSON via `serde_json`, but the rules apply to any format serde supports (YAML, TOML, bincode, MessagePack, etc.).

## Unknown fields — silently ignored by default

JSON can have fields your struct doesn't name. Serde ignores them.

GitHub's `/repos/rust-lang/rust` returns ~100 fields. Our struct names 5. The other ~95 are read and discarded — no error.

```rust
#[derive(Deserialize)]
struct Repo {
    full_name: String,
    stargazers_count: u32,
    // created_at, license, topics, default_branch, ... all ignored
}
```

This is almost always what you want — APIs evolve, add fields, and your code shouldn't break when they do.

### When you want the opposite

Use `#[serde(deny_unknown_fields)]`:

```rust
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config { /* ... */ }
```

Now any JSON field not named in the struct causes a parse error. Useful for:

- **Config files** — catch typos like `timout` vs `timeout`.
- **Protocol parsers** — catch API contract drift.

Don't use it on external APIs you don't control — the moment the API adds a field, your parsing breaks.

## Missing fields — error by default

If your struct declares a field and the JSON lacks it, parsing fails:

```
Error: missing field `stargazers_count` at line 1 column 2345
```

Three ways to handle fields that might be missing:

**1. Make the field optional with `Option<T>`.**

```rust
description: Option<String>,    // None if absent or null
```

**2. Use `#[serde(default)]` to fill in `Default::default()`.**

```rust
#[serde(default)]
stargazers_count: u32,          // 0 if missing
```

Default value comes from the type's `Default` impl: `0` for numbers, `""` for strings, `Vec::new()` for vecs.

**3. Use a custom default function.**

```rust
#[serde(default = "default_limit")]
limit: u32,

fn default_limit() -> u32 { 100 }
```

## Full behavior table

| JSON state | Field type | Result |
|---|---|---|
| `"abc"` | `String` | `"abc".to_string()` |
| `null` | `String` | error |
| `null` | `Option<String>` | `None` |
| (missing) | `String` | error |
| (missing) | `String` + `#[serde(default)]` | `""` |
| (missing) | `Option<String>` | `None` |
| (missing) | `Option<String>` + `#[serde(default)]` | `None` (same as just `Option<String>`) |

## `null` vs missing — Rust collapses them

JSON has two ways to say "no value":

- `null` — explicit literal
- *missing from the object* — what JS/TS calls `undefined`

TypeScript treats these as separate states:

```typescript
{ a: string | null | undefined }   // three states to worry about
```

Rust's `Option<T>` collapses both into `None`:

```rust
a: Option<String>
```

| JSON | Rust |
|---|---|
| `"a": "hello"` | `Some("hello".to_string())` |
| `"a": null` | `None` |
| (field missing) | `None` |

Simpler, fewer edge cases. You trade a little expressiveness for a lot of safety.

### When you need to distinguish null from missing

Rare, but possible with nested `Option`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
description: Option<Option<String>>,
```

| JSON | Rust |
|---|---|
| `"description": "hello"` | `Some(Some("hello"))` — present, has value |
| `"description": null` | `Some(None)` — present, explicitly null |
| (missing) | `None` — absent entirely |

Most APIs don't distinguish the two in ways your code needs to care about. Don't reach for this unless you have a concrete reason.

## Why `.json()` is async

The parsing is synchronous. The `.await` is for reading the body bytes.

After `response.send().await?` returns, you have the **headers** (status, content-type, content-length). The **body** is still arriving over the network. `.json()` does two things:

1. **Read the body** — `.await` until all bytes arrive (slow, async, I/O-bound).
2. **Parse the bytes as JSON** — fast, sync, CPU-bound.

Step 2 can't start until step 1 finishes, so the combined operation is async overall.

In slower motion:

```rust
let bytes = response.bytes().await?;                       // async — read from network
let repo: Repo = serde_json::from_slice(&bytes)?;          // sync — parse
```

`.json::<T>().await?` fuses these. Either form works; `.json()` is more ergonomic.

## Streaming preview (CP5)

At CP5 the same split matters more:

```rust
let mut stream = response.bytes_stream();           // receive chunks as they arrive
while let Some(chunk) = stream.next().await {
    let bytes = chunk?;
    // parse JSON events out of `bytes` as they come in
}
```

`.bytes_stream()` returns a `Stream<Item = Result<Bytes, ...>>`. Each `.await` pulls one chunk. Serde still parses each chunk's JSON — same tool, smaller and more frequent applications, typically via an enum like:

```rust
#[derive(Deserialize)]
#[serde(tag = "type")]
enum StreamEvent {
    MessageStart { message: Message },
    ContentBlockDelta { index: u32, delta: Delta },
    MessageStop,
    // ...
}
```

## Takeaway

- **Unknown fields:** ignored by default. `#[serde(deny_unknown_fields)]` makes them errors.
- **Missing fields:** error by default. Use `Option<T>`, `#[serde(default)]`, or a custom default function.
- **`Option<T>` catches both `null` and missing** — two JSON concepts collapsed into one Rust type.
- **`.json()` is async** because reading the body is I/O. Parsing itself is sync.
- **The pattern scales to streaming** — same serde derive, smaller event-shaped enums, parsed as chunks arrive instead of all at once.
