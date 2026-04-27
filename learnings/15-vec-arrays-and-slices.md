# Vec, arrays, and slices — Rust's three "array-like" types

A common confusion coming from JavaScript, Python, or Java: what those languages call "array" or "list" or "ArrayList" is one type. Rust splits the same concept into **three distinct types** with different tradeoffs. Knowing the difference clears up a lot of "which type should this parameter be?" questions.

## The three types at a glance

| Type | Size | Where it lives | Growable? | Closest mainstream equivalent |
|---|---|---|---|---|
| `Vec<T>` | dynamic | heap | ✅ yes | JS `Array`, Python `list`, Java `ArrayList`, C# `List<T>` |
| `[T; N]` | fixed at compile time | stack (usually) | ❌ no | C `int arr[N]`, Java fixed `int[]` |
| `&[T]` | dynamic | borrowed view | n/a (read-only by default) | Java `List<T>` parameter, slice in Go |

`Vec<T>` is what you almost always reach for. The fixed-size array `[T; N]` and the slice `&[T]` come up in specific situations.

## `Vec<T>` — the dynamic array

Owned, heap-allocated, can grow and shrink:

```rust
let mut v: Vec<i32> = Vec::new();   // empty, capacity 0
v.push(1);
v.push(2);
v.push(3);
v.len();                             // 3
v[0];                                // 1 — index access
```

This is the JS `Array` / Python `list` shape. Growable, ordered, indexable.

## `[T; N]` — the fixed-size array

Size baked into the type at compile time. Lives on the stack (usually):

```rust
let arr: [i32; 5] = [10, 20, 30, 40, 50];
arr.len();                           // 5 — always 5
// arr.push(60);                     // ERROR: arrays don't have .push()
```

The size `N` is part of the type itself. `[i32; 5]` and `[i32; 6]` are different types. You can't grow it.

When to use: small collections whose size is known at compile time. RGB triples, fixed lookup tables, small buffers, embedded code. Rare in app code; common in systems code.

## `&[T]` — the slice (borrowed view)

A reference to a contiguous sequence. Doesn't own the data — just looks at it:

```rust
let v: Vec<i32> = vec![1, 2, 3, 4, 5];
let s: &[i32] = &v;                  // slice over the whole Vec
let s2: &[i32] = &v[1..4];           // slice over elements 1..4
```

A `&[T]` parameter is the "I want to read a sequence" type — accepts `&Vec<T>` (via deref coercion) and `&[T; N]` (via coercion).

Hence the convention from `learnings/14`: `fn foo(messages: &[Message])` is more general than `fn foo(messages: &Vec<Message>)`. That's why cawir's `ask_claude` takes `&[Message]`, not `&Vec<Message>`.

## The `vec!` macro

A macro that constructs a `Vec<T>`. The trailing `!` is Rust's "this is a macro" indicator (see `learnings/03e-functions-and-macros.md` and `11-derive-macros.md`).

```rust
let v = vec![1, 2, 3];        // Vec<i32> with 3 elements
let v: Vec<u8> = vec![];      // empty Vec
let v = vec![0; 100];         // Vec with 100 zeros (the "fill" form)
```

Why a macro, not a function:

- Needs variadic arguments — Rust functions can't be variadic without macros.
- The `[expr; N]` "fill" syntax isn't a function-call expression.
- Macros generate the right code at compile time, including pre-allocation.

`vec![1, 2, 3]` typically expands to roughly:

```rust
{
    let mut temp = Vec::with_capacity(3);
    temp.push(1);
    temp.push(2);
    temp.push(3);
    temp
}
```

It pre-allocates capacity sized to fit the elements — slightly more efficient than `Vec::new()` followed by three `push` calls.

## `Vec::new()` vs `vec![]` — same thing

Both create an empty `Vec<T>` with no heap allocation:

```rust
let v: Vec<i32> = Vec::new();
let v: Vec<i32> = vec![];
```

Identical in behavior. Style preference:

- `Vec::new()` reads as "make a new Vec" — preferred when you'll fill incrementally.
- `vec![]` is shorter — fits when nearby code uses literal-form construction like `vec![1, 2, 3]`.

## Why cawir uses `Vec` and not `[Message; N]`

The number of conversation turns is unknown at compile time — the user types as many turns as they want. A fixed-size `[Message; 100]` would either fail on turn 101 or waste space.

Use `[T; N]` only when the size is genuinely fixed:

```rust
let rgb: [u8; 3] = [255, 100, 50];           // colors are always 3 bytes
let coords: [f64; 2] = [37.7, -122.4];       // lat/long is always 2 floats
```

These are exceptions. Default to `Vec` for any sequence whose length isn't known up front.

## Mental model from JS / Python / Java

```
JavaScript / Python / Java                    Rust
─────────────────────────────────              ─────────────────────────
Array / list / ArrayList                  →    Vec<T>      (heap, growable)
fixed-size primitive array (rare)         →    [T; N]      (stack, fixed)
"a piece of an array" (no direct type)    →    &[T]        (borrowed slice)
```

Rust's distinction comes from caring precisely about ownership and where data lives. Most of the time you reach for `Vec<T>`, and the construction macro is `vec!`. The other two types matter when you want stack allocation (`[T; N]`) or when you want to write a function that accepts any sequence without taking ownership (`&[T]`).

## Common `Vec<T>` operations

You'll use these constantly:

```rust
let mut v: Vec<i32> = Vec::new();

v.push(1);                  // append
v.pop();                    // remove last — returns Option<T>
v.len();                    // length — usize
v.is_empty();               // bool

v[0];                       // index — panics if out of bounds
v.get(0);                   // safer index — Option<&T>

v.first();                  // Option<&T>
v.last();                   // Option<&T>

for x in &v { ... }         // iterate by shared ref
for x in &mut v { ... }     // iterate by mutable ref
for x in v { ... }          // iterate by value (consumes v)

v.iter().map(|x| x * 2).collect::<Vec<_>>();         // transform
v.iter().filter(|&&x| x > 0).collect::<Vec<_>>();    // filter
```

The iteration methods (`.iter()`, `.map()`, `.filter()`, `.collect()`) are entry points into Rust's `Iterator` trait, used heavily from CP3's tool dispatch through CP5's stream processing.

## See also

- `learnings/05-ownership-and-borrowing.md` — `mut`, `&`, `&mut`, and the borrowing rules.
- `learnings/14-function-parameters-and-ownership.md` — why `&[T]` is preferred over `&Vec<T>` in function parameters.
