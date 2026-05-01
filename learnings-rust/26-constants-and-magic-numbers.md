# Constants and magic numbers

Checkpoint 3h raised Anthropic's output-token budget from an inline `1024` to a named constant:

```rust
const MAX_OUTPUT_TOKENS: u32 = 16_384;
```

and then used it in the request:

```rust
max_tokens: MAX_OUTPUT_TOKENS,
```

## Why a constant instead of an inline number

An inline number is easy to write:

```rust
max_tokens: 1024,
```

But once the value has project meaning, a name is better:

```rust
MAX_OUTPUT_TOKENS
```

The name records the reason for the number. It also gives future config extraction an obvious place to start.

## `const` basics

Rust constants:

- are introduced with `const`
- require an explicit type
- are usually named in `SCREAMING_SNAKE_CASE`
- can be used wherever the value is needed

Example:

```rust
const MAX_TOOL_ROUNDS: usize = 42;
const MAX_OUTPUT_TOKENS: u32 = 16_384;
```

The type matters. `MAX_TOOL_ROUNDS` is a `usize` because it is compared with a loop counter. `MAX_OUTPUT_TOKENS` is a `u32` because the request struct's `max_tokens` field is a `u32`.

## Numeric separators

Rust allows underscores inside numeric literals:

```rust
16_384
```

This is the same value as:

```rust
16384
```

The underscore is only for readability. It is common for large numbers, token counts, byte counts, and bit-oriented values.

## Takeaway

Use inline numbers while sketching. Promote them to named constants once they represent a policy choice, API limit, safety cap, or tuning knob.
