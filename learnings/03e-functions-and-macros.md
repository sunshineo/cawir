# `fn main()`, `println!`, and macros vs functions

## Anatomy of the hello world

```rust
fn main() {
    println!("Hello, world!");
}
```

These four lines introduce several foundational Rust concepts.

### `fn main()`

- **`fn`** — function declaration keyword. Short for "function." (`def` in Python, `function` in JS.)
- **`main`** — conventional entry point. Rust looks for a function called `main` in the binary crate root (`src/main.rs`, or wherever you configured — see `03d-crate-layout.md`).
- **`()`** — parameter list. Empty here. Unlike C/Java, `main` doesn't take `argc`/`argv` — command-line args are accessed via `std::env::args()` inside the body.
- **No return type arrow `->`** — so `main` implicitly returns `()`, Rust's **unit type** (equivalent to `void` in C/Java, or `None` being the only return in Python).

### Alternative: `fn main() -> Result<(), E>`

`main` can also be declared to return `Result<(), E>`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let x = some_fallible_thing()?;
    Ok(())
}
```

This lets you use `?` inside `main` to bubble up errors — the `?` operator propagates `Err` values up the call stack. If `main` returns `Err`, the program exits with a non-zero status and prints the error via `Debug`. We'll likely adopt this pattern in cawir once we start doing I/O.

### `println!("Hello, world!")`

- **`println!`** — a **macro**, not a function. The trailing `!` is always how Rust spells macro invocations. If you ever see `foo!(...)` or `foo![...]` or `foo!{...}`, it's a macro.
- **`"Hello, world!"`** — a string literal. Its type is `&'static str` — a reference to a UTF-8 string slice that's baked into the binary. Don't worry about the lifetime annotation `'static` yet; it just means "this data lives for the whole program."

## Macros you'll meet early

| Macro | What it does |
|---|---|
| `println!` | Format + print to stdout with newline. |
| `print!` | Like `println!` but no trailing newline. |
| `eprintln!` / `eprint!` | Write to stderr instead of stdout. |
| `format!` | Returns a `String` instead of printing. Used constantly — the normal way to build a formatted string. |
| `write!` / `writeln!` | Write into any type implementing the `Write` trait (a file, a buffer, a socket). |
| `dbg!` | Quick-and-dirty debug print that returns the value so you can wrap expressions inline: `let x = dbg!(5 + 3);` prints the expression AND its value, returns `x = 8`. |
| `vec!` | Terse `Vec` constructor: `vec![1, 2, 3]`. |
| `assert!`, `assert_eq!`, `assert_ne!` | Test assertions. |
| `unimplemented!`, `todo!`, `unreachable!` | Panic markers for stub or unreachable code. |

## Deep dive — macro vs function

Best single-line version: **a function takes values and returns a value; a macro takes code and generates more code.**

They look almost identical when called — `foo(x)` vs `foo!(x)` — but what happens next is completely different.

### Functions run at runtime. Macros run at compile time.

```rust
// Function call
let n = add(2, 3);      // At runtime: 2 and 3 are passed in, add returns 5.

// Macro call
println!("n = {}", n);  // At COMPILE TIME: macro expands into real Rust code.
                        // At runtime: only the expanded code runs.
```

When the compiler sees `println!("n = {}", n)`, it literally rewrites that line into something roughly like:

```rust
{
    let args = format_args!("n = {}", n);   // build a formatter
    ::std::io::_print(args);                // hand it to stdout
}
```

That rewritten code is what actually runs. `println!` itself doesn't exist at runtime — it's gone by the time your program starts.

### The 5 superpowers of macros

What macros can do that functions can't:

#### 1. Variable number of arguments

Rust functions have a **fixed** parameter list. But these all work:

```rust
println!("hello");
println!("hello {}", name);
println!("hello {} you are {} years old", name, age);
```

Functions **can't** do that. A macro can, because it generates different code for each invocation.

#### 2. Compile-time format-string checking

```rust
println!("{} and {}", x);   // ← COMPILE ERROR: 2 placeholders, 1 arg
```

That's caught before your program ever runs. A function receiving a format string as a `&str` couldn't check it until runtime — by which point it's too late.

#### 3. Taking code, not values

```rust
let n = 42;
dbg!(n * 2);
// Prints:  [src/main.rs:3] n * 2 = 84
```

`dbg!` printed both the **source text** `"n * 2"` *and* its value. A function can't do that — by the time a function runs, it only has the value `84`, not the original expression.

#### 4. Generating types and impls

```rust
#[derive(Debug, Clone, PartialEq)]
struct User { name: String, age: u32 }
```

`derive` is a macro. It writes out the `impl Debug for User { ... }`, `impl Clone for User { ... }`, etc., automatically. You get hundreds of lines of code from one line. A function can't generate code.

#### 5. Terse literal constructors

```rust
let v = vec![1, 2, 3, 4, 5];
// Expands to roughly:
//   let mut temp = Vec::new();
//   temp.push(1); temp.push(2); temp.push(3); ...
//   temp
```

`vec!` is a macro because it needs to accept `[1, 2, 3]` or `[0; 1000]` (a thousand zeros) or `[x, y, z]` — all different shapes. Functions can't have that flexibility.

### The syntactic signal

Rust always requires you to mark macro calls with `!`. That's how the compiler (and you, reading code) distinguish:

```rust
foo(x)      // function — runtime
foo!(x)     // macro    — compile-time expansion
```

Macros also accept any bracket style, all equivalent:

```rust
vec![1, 2, 3]
vec!(1, 2, 3)
vec!{1, 2, 3}
```

Convention: `vec!` uses `[]`, `println!` uses `()`, `macro_rules!` definitions use `{}`. No rule enforces this.

### TS/Python analogues (approximate)

| Rust | Closest in other languages |
|---|---|
| Macro | Python **decorators** (`@dataclass`, `@property`) transform code at definition time — similar spirit. TS **decorators** too. But Rust macros are more powerful — they see arbitrary syntax. |
| Macro | C **preprocessor** (`#define`) is the historical comparison, but C's preprocessor is dumb text substitution. Rust macros operate on the parsed syntax tree, so they understand structure and can't produce garbage. |
| Macro | JS — no real equivalent. Tagged template literals (`` html`<div>${x}</div>` ``) are vaguely similar in that a function sees the raw parts, but they run at runtime, not compile time. |

### When do *you* write a macro?

Probably not for a long time. Use the built-ins (`println!`, `format!`, `vec!`, `dbg!`, `assert!`, etc.) freely — they're your main exposure to macros. Writing your own is an advanced topic and usually a signal you should first ask "would a function or trait work instead?" The Rust community's rough rule: **prefer functions and traits; reach for macros when you literally can't express it otherwise.**

### Mental heuristic

When you see `foo!(...)`, read it as: *"the compiler is going to rewrite this line into some other Rust code for me."*

That's the whole concept. Everything else is flavor.
