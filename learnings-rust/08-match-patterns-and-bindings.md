# `match` patterns and bindings

`match` is Rust's primary control-flow and destructuring tool. It combines switch-statement dispatch with pattern matching and data extraction. The key thing to understand: a match arm's left-hand side is a **pattern**, and different pattern shapes mean different things.

## The three shapes of match patterns

| Pattern | Meaning | Binds a value? |
|---|---|---|
| `"/exit"` or `42` or `true` — a **literal** | Match only if the scrutinee equals this value | No — pure equality check |
| `_` — the **underscore** | Match anything | No — discards the value |
| `other` — a **bare identifier** | Match anything | **Yes** — binds matched value to a new local variable of that name |

From cawir's 1c:

```rust
match trimmed {
    "" => continue,                    // literal — match empty string exactly
    "/exit" => break,                  // literal
    "/help" => print_help(),           // literal
    other => {                         // identifier — binds matched value to `other`
        if other.starts_with('/') { ... }
    }
}
```

`other` is **not a keyword**. It's a variable name picked by convention. Could be `x`, `input`, `cmd`, `whatever` — the compiler doesn't care.

## Bare identifier binding — the critical rule

When you write a lowercase name by itself as a pattern, it's a **new variable binding** — not a comparison to an existing variable. Even if a variable with that name already exists in the outer scope, the match arm's identifier is fresh.

```rust
let name = "Alice";

let x = "Bob";
match x {
    name => println!("matched: {}", name),  // prints "matched: Bob"
}
// outer `name` is still "Alice"
```

The `name` in the arm shadows the outer `name` for the duration of the arm. It gets the value of `x`, not the value of outer `name`.

## Conventions that prevent confusion

Rust treats identifiers in patterns differently based on their casing:

| Naming style | Rust treats it as |
|---|---|
| `snake_case` / single lowercase word | **new binding** (matches anything, binds the value) |
| `UPPER_CASE` | **constant lookup** (matches against the const's value) |
| `PascalCase` | **enum variant or type** (e.g. `Ok`, `Err`, `Some`, `None`) |

Example of the `UPPER_CASE` rule biting you:

```rust
const EXIT: &str = "/exit";

match trimmed {
    EXIT => break,      // matches against the const's value "/exit"
    other => ...,
}
```

vs. the naming collision with a local variable:

```rust
let exit_cmd = "/exit";

match trimmed {
    exit_cmd => break,  // this is a NEW BINDING — matches everything, equal to trimmed
    other => ...,       // unreachable! compiler warns
}
```

Stick with conventions: constants `UPPER_CASE`, variables `snake_case`. Saves you.

## Outer bindings inside match arms

Whether outer variables stay accessible inside a match arm depends on whether the match *moves* ownership of the scrutinee.

### Copy types — no move, outer still accessible

```rust
let trimmed: &str = ...;
match trimmed {
    other => {
        println!("{}", other);    // works
        println!("{}", trimmed);  // also works — &str is Copy
    }
}
```

`&str` is `Copy` (cheap to duplicate). Matching on it doesn't move; the outer binding is still live.

### Non-Copy types — moved into the arm's binding

```rust
let s: String = String::from("hi");
match s {
    x => {
        println!("{}", x);      // fine — s moved into x
        // println!("{}", s);   // ERROR: s was moved
    }
}
```

`String` isn't `Copy`. Matching moves it into `x`. The outer `s` is no longer usable.

**Workaround:** match on a reference to preserve the outer binding.

```rust
match &s {
    x => {
        println!("{}", x);    // x is &String
        println!("{}", s);    // still fine — we matched a reference, not the value
    }
}
```

This is a common pattern.

### Destructuring patterns

For enums and structs, matching often *destructures*:

```rust
let result: Result<i32, String> = Ok(42);
match result {
    Ok(n) => { /* n is the inner i32 */ }
    Err(e) => { /* e is the inner String */ }
}
```

Whether the outer `result` remains accessible after the match depends on whether the destructured fields are `Copy`. `i32` is Copy, `String` is not. Typically you just don't reference the outer after destructuring.

## Using `_` vs a bare identifier

Use `_` when you want to match anything but don't need the value:

```rust
match trimmed {
    "" => continue,
    "/exit" => break,
    _ => println!("some other input"),   // can't access the value
}
```

Use a bare identifier when you *do* need the value:

```rust
match trimmed {
    "" => continue,
    "/exit" => break,
    other => println!("got: {}", other),   // value is accessible
}
```

Rust warns about unused bindings — but *not* if the name starts with `_`:

```rust
other => { }     // warning: `other` is never used
_other => { }    // no warning — leading _ means "intentionally unused"
_ => { }         // no warning — fully anonymous
```

## Arm order matters

Match arms are tried top-to-bottom. More specific (literal) patterns must come before catch-all (identifier or `_`) patterns, or the catch-all will consume everything and later arms become unreachable:

```rust
match trimmed {
    other => println!("{}", other),   // catches EVERYTHING
    "/exit" => break,                 // unreachable — compiler warns
}
```

The compiler catches this at compile time via the unreachable-pattern warning.

## Exhaustiveness — match must cover every case

Rust requires `match` to handle every possible value of the scrutinee. For types with infinite possible values (like `&str` or `u32`), you need a catch-all (`_` or a binding identifier). For enums with a closed variant set, the compiler tracks whether you've covered them all:

```rust
enum Mode { Default, Plan, Bypass }

fn describe(m: Mode) -> &'static str {
    match m {
        Mode::Default => "default",
        Mode::Plan => "plan",
        // ERROR: non-exhaustive — missing Mode::Bypass
    }
}
```

Adding a catch-all fixes the compile error but defeats the exhaustiveness check — now adding a new variant won't force you to handle it. For enum matching, prefer listing each variant over using `_`.

## Takeaway

- Literals match by equality; bare identifiers match anything *and bind*; `_` matches anything and discards.
- A bare identifier in a match pattern is a **new binding**, regardless of whether a variable of that name already exists outside.
- Conventions: `snake_case` = binding, `UPPER_CASE` = const lookup, `PascalCase` = enum variant.
- Outer bindings remain accessible inside arms for `Copy` types; non-`Copy` values get moved into the arm's binding.
- More-specific patterns must precede catch-alls.
- Match must be exhaustive.
