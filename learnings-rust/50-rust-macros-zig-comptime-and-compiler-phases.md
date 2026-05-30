# Rust macros, Zig comptime, and compiler phases

This note summarizes a discussion that started with "what is Zig?" and narrowed into why Rust has macros, why `println!` cannot be an ordinary function, and how Zig's `comptime` differs from Rust's macro system.

Related notes:

- `03e-functions-and-macros.md`
- `11-derive-macros.md`
- `05-ownership-and-borrowing.md`
- `06-traits-and-scope.md`

## Rust, Zig, and C at a high level

Rust, Zig, and C are all systems languages without a garbage collector, but they choose different safety and language-design tradeoffs.

| Area | C | Zig | Rust |
|---|---|---|---|
| Memory management | Manual `malloc` / `free` | Manual, usually allocator-explicit | Ownership and borrowing, automatic cleanup via `Drop` |
| Garbage collector | No | No | No |
| Memory safety | Programmer responsibility | Programmer responsibility with better checks/defaults than C | Enforced in safe Rust by the compiler |
| Ownership model | Convention | Convention/API design | Core language feature |
| Null handling | Raw nullable pointers | Explicit optional types like `?T` | Explicit `Option<T>` |
| Error handling | Conventions: `NULL`, `-1`, `errno` | Error unions like `!T` | `Result<T, E>` |
| Cleanup | Manual, often `goto cleanup` | `defer` / `errdefer` | RAII through `Drop` |
| Generics | Macros, duplicated code, or `void *` | `comptime` type parameters | Generics plus traits |
| Metaprogramming | Preprocessor macros | Compile-time Zig execution | Macros, derives, const eval, traits, monomorphization |
| Build tooling | External tools like Make/CMake | Built-in `zig build` | Cargo |
| C interop | Native | Excellent | Good, but more ceremony |

The useful spectrum:

```text
C      -> maximum tradition/control, fewest guardrails
Zig    -> C-like control, cleaner language/toolchain
Rust   -> systems performance, strongest compile-time safety model
```

For someone coming from Java, TypeScript, or Python, Rust is already a serious step downward in runtime abstraction because there is no garbage collector. Python does have garbage collection: CPython mainly uses reference counting, plus a cyclic garbage collector for reference cycles. "Interpreted" and "garbage-collected" are separate concepts.

The practical learning recommendation was: keep building `cawir` in Rust, and later create a separate tiny Zig project if desired. C is useful historically, but C is not a necessary prerequisite for Zig. Zig is often a better way to learn many C-level ideas with clearer language tools.

## Generics vs metaprogramming

Generics and metaprogramming are related, but not the same.

```text
Generics:
  Write one function or type that works with many concrete types.

Metaprogramming:
  Write code that runs during compilation to generate, inspect, validate,
  or specialize code/types.
```

Example generic idea in TypeScript:

```ts
function first<T>(items: T[]): T | undefined {
  return items[0]
}
```

Rust version:

```rust
fn first<T>(items: &[T]) -> Option<&T> {
    items.first()
}
```

Zig version:

```zig
fn first(comptime T: type, items: []const T) ?T {
    if (items.len == 0) return null;
    return items[0];
}
```

In Rust, a trait bound can say what behavior a generic type must provide:

```rust
trait ToPrompt {
    fn to_prompt(&self) -> String;
}

fn render<T: ToPrompt>(value: T) -> String {
    value.to_prompt()
}
```

That is ordinary abstraction, not necessarily metaprogramming. Metaprogramming shows up when the compiler generates/checks code based on syntax or type structure.

## Why `println!` is a macro

This compiles:

```rust
println!("hello {} {}", name, age);
```

This fails at compile time:

```rust
println!("hello {} {}", name);
```

The key question was: why can't `println` just be a function?

A function receives values after compilation has already accepted the program. For example:

```rust
fn println_func(format: &str, arg: impl std::fmt::Display) {
    // This can parse `format`, but only at runtime.
}
```

If called like this:

```rust
println_func("hello {} {}", name);
```

the function sees:

```text
format = "hello {} {}"
arg = name
```

It can count placeholders at runtime and maybe panic, but it is too late to produce a compile error. Also, ordinary Rust functions do not support a variable number of typed arguments like C's `printf`.

A macro sees source-code tokens during compilation. `println!` is roughly:

```rust
macro_rules! println {
    () => {
        print!("\n")
    };

    ($($arg:tt)*) => {{
        std::io::_print(format_args_nl!($($arg)*));
    }};
}
```

The `println!` macro mostly forwards tokens to `format_args_nl!`. The special part is `format_args!` / `format_args_nl!`, which is compiler-supported. It can inspect a string literal token like:

```rust
"hello {} {}\n"
```

and count the placeholders before normal runtime code is produced.

Conceptually:

```rust
format_args!("hello {} {}\n", name)
```

is handled by compiler logic like:

```rust
fn expand_format_args(input_tokens: TokenStream) -> TokenStream {
    let (format_literal, value_args) = parse_macro_input(input_tokens);
    let pieces = parse_format_string(format_literal);

    let placeholder_count = pieces
        .iter()
        .filter(|piece| piece.is_placeholder())
        .count();

    if placeholder_count != value_args.len() {
        return compiler_error(
            "2 positional arguments in format string, but there is 1 argument",
        );
    }

    generate_arguments_code(pieces, value_args)
}
```

That pseudo-code is not inside the program being compiled. It represents compiler/macro-expander behavior. The boundary is:

```text
Compiler world:
  runs while compiling the program
  sees source-code tokens
  can emit generated Rust code or compiler diagnostics

Program world:
  runs after compilation
  sees runtime values
  functions receive values, not source-code tokens
```

The compact rule:

```text
A function can use data.
A macro can use code-as-data.
```

## Token trees, ASTs, and LLVM

Rust macros are not plain text substitution like C preprocessor macros. They operate on token trees / syntax-like structures.

For:

```rust
println!("hello {}", name);
```

the compiler sees tokens roughly like:

```text
identifier: println
punctuation: !
group: (
  string literal: "hello {}"
  punctuation: ,
  identifier: name
)
```

This happens early in compilation, long before LLVM. LLVM is a backend for optimization and code generation. It does not know about Rust macros, `println!`, derives, borrow checking, or traits in their original source-level form.

Simplified Rust pipeline:

```text
source text
  -> lexing: characters -> tokens
  -> parsing: tokens -> syntax tree / AST-like structures
  -> macro expansion
  -> name resolution + type checking
  -> Rust MIR
  -> LLVM IR or another backend representation
  -> machine code
```

## Rust compile-time mechanisms

The earlier short list was not complete. Rust has several different compile-time mechanisms:

| Mechanism | What it does |
|---|---|
| `macro_rules!` | Declarative, pattern-based token expansion |
| Function-like procedural macros | Custom token-stream transformation: `my_macro!(...)` |
| Derive procedural macros | Generate code from item structure: `#[derive(Foo)]` |
| Attribute procedural macros | Transform annotated items: `#[route("/")] fn f() {}` |
| Built-in macros | Compiler-provided macros like `format_args!`, `include_str!`, `concat!`, `env!` |
| `const fn` | Functions that can run during compile-time evaluation |
| `const` / associated consts | Compile-time constants |
| Const generics | Values as generic parameters, like `[u8; N]` |
| Conditional compilation | `#[cfg(...)]`, `cfg!`, Cargo features |
| Trait resolution | Compile-time selection/checking of trait implementations |
| Monomorphization | Compiler creates concrete versions of generic code |
| Build scripts | `build.rs` runs before compiling the crate |

This is why Rust's compile-time story can feel fragmented. There are macros, derives, compiler built-ins, const evaluation, trait resolution, Cargo features, and build scripts. They are related in practice, but they are not one unified feature.

## Derive macros and Java annotations

Rust derive macros look similar to Java annotations:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}
```

```java
@Entity
public class Message {
    private String role;
    private String content;
}
```

But the mechanics differ.

Java annotations are usually metadata. A compiler, annotation processor, runtime framework, or reflection-based tool can read that metadata.

Rust derive macros generate Rust code. For example:

```rust
#[derive(Debug)]
struct Message {
    role: String,
    content: String,
}
```

generates something conceptually like:

```rust
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // generated field-by-field formatting code
    }
}
```

The closer Java analogy is Lombok or annotation processing that generates code:

```java
@Data
public class User {
    private String name;
}
```

Rust derives usually generate trait implementations. That matters because once the derive has generated:

```rust
impl Serialize for Message {
    // ...
}
```

the rest of the Rust ecosystem can use `Message` anywhere a `Serialize` trait bound is required:

```rust
fn save<T: Serialize>(value: &T) {
    // ...
}
```

So a derive macro does not merely attach metadata. It creates real code that the compiler later type-checks.

## Zig's compiler pipeline

Zig has the normal compiler front-end ideas too: source text becomes tokens, tokens become an AST, and later compiler phases analyze and lower that representation.

Simplified Zig pipeline:

```text
Zig source text
  -> lexing
  -> parsing
  -> AST
  -> AstGen
  -> ZIR
  -> Sema
  -> AIR
  -> codegen backend
  -> object code / machine code
  -> linking
  -> executable
```

Zig-specific pieces:

```text
ZIR = Zig Intermediate Representation
AIR = Analyzed Intermediate Representation
Sema = semantic analysis phase
```

More detail:

```text
Zig source
  -> tokenizer
  -> parser
  -> AST
  -> AstGen
  -> ZIR: untyped, lower-level representation of Zig code
  -> Sema:
       name resolution
       type checking
       comptime execution
       generic specialization
       safety-check generation
  -> AIR: typed/analyzed representation
  -> codegen:
       LLVM backend, or
       Zig native backend, or
       C backend, depending on target/options
  -> object files
  -> linker
  -> binary
```

## Where Zig `comptime` runs

Zig `comptime` code runs during `Sema`.

That means it happens after parsing and after AST-to-ZIR lowering:

```text
source
  -> AST
  -> ZIR
  -> Sema runs comptime code
  -> AIR
  -> codegen
```

This is the key contrast:

```text
Rust macro expansion:
  before type checking
  transforms token trees / syntax into more Rust syntax

Zig comptime:
  during semantic analysis
  evaluates normal Zig code while resolving types and producing AIR
```

Example:

```zig
fn add(comptime T: type, a: T, b: T) T {
    return a + b;
}

const x = add(i32, 1, 2);
```

During `Sema`, Zig can determine that `T = i32`, specialize the function, and evaluate compile-time-known pieces.

Another example:

```zig
const n = comptime blk: {
    var x = 1;
    x += 2;
    break :blk x;
};
```

During semantic analysis, Zig evaluates the block and determines that `n` is `3`.

## Does Zig `comptime` modify the AST?

No, not in the Rust macro sense.

By the time `comptime` runs, the compiler is already past the AST:

```text
source
  -> AST
  -> ZIR
  -> Sema / comptime
```

Zig `comptime` does not take arbitrary syntax tokens and return different syntax tokens. It does not primarily work by source expansion.

It can:

- compute values
- choose branches
- construct types
- inspect types
- instantiate generic functions
- cause compile errors
- decide what runtime AIR/code gets produced

It does not:

- rewrite arbitrary source syntax
- inject token streams like a Rust procedural macro
- generate trait impls in the Rust derive-macro sense

Useful contrast:

```text
Rust macro:
  input: tokens / syntax
  output: tokens / syntax
  effect: expands program structure before type checking

Zig comptime:
  input: normal Zig values/types known during compilation
  output: values/types/selected code paths/compile errors
  effect: influences semantic analysis and code generation
```

Short version:

```text
Rust macros can invent syntax-shaped code.
Zig comptime can invent values and types, and specialize normal code.
```

## Why Rust can be more convenient in some cases

For serialization, Rust can be very convenient:

```rust
#[derive(Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}
```

The derive macro generates `impl Serialize for Message` and `impl Deserialize for Message`. The compiler then sees those impls as real code, so trait bounds work across the ecosystem.

In Zig, a JSON function can often inspect the type at compile time:

```zig
try std.json.stringify(message, .{}, writer);
```

The function can use `@TypeOf(message)` and compile-time reflection to serialize the fields. That is more unified conceptually because it is normal Zig code. But Zig does not have Rust-style traits, so there is not necessarily a separate ecosystem-wide statement like:

```text
Message implements Serialize
```

The distinction:

```text
Rust derive macro:
  generates an impl
  the type gains a named trait capability
  other code can require that capability with trait bounds

Zig comptime:
  generic code inspects/specializes based on the type
  compatibility is often checked when the generic function is instantiated
```

## Main takeaways

- Macros are not just shortcuts. They let the compiler operate on code as data.
- `println!` cannot be a normal Rust function because functions receive runtime values, not source tokens, and Rust functions do not have type-safe variadic arguments.
- `format_args!` is compiler-supported, so it can parse string literal tokens and emit compile errors before runtime.
- Rust macro expansion happens early, before type checking and long before LLVM.
- Rust derive macros generate real Rust code, usually trait impls.
- Java annotations are usually metadata; Rust derive macros are closer to Java annotation processors or Lombok because they generate code.
- Zig `comptime` is more unified than Rust macros: it runs normal Zig code during semantic analysis.
- Zig `comptime` does not rewrite the AST like Rust macros rewrite syntax. It computes values/types, specializes code, and can emit compile errors.
- Rust's system is more fragmented and harder to learn, but it enables very powerful library ergonomics.
- Zig's system is more conceptually elegant, but less like "attach a trait capability to this type for the whole ecosystem."
