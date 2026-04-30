# `OsString`, `String`, and lossy filename conversion

Checkpoint 3d also surfaced a very common filesystem question:

```rust
let mut name = entry.file_name().to_string_lossy().into_owned();
```

Why not just get a `String` directly?

Because Rust does **not** assume filenames are valid UTF-8.

## `String` is UTF-8 text

Rust's `String` type always contains valid UTF-8.

That is a stronger guarantee than "some bytes that look like text." It means Rust can safely support string operations without constantly re-checking encoding validity.

So if a value is a `String`, Rust is promising:

> this is valid UTF-8 text

## Filenames are OS-native, not guaranteed UTF-8

Filesystem names come from the operating system, not from Rust's text model.

That is why `DirEntry::file_name()` returns:

```rust
OsString
```

instead of:

```rust
String
```

`OsString` means:

> a platform-native owned string for OS paths and process arguments

It may be valid UTF-8, but Rust cannot assume that up front.

This is especially important on Unix-like systems, where filenames are effectively arbitrary bytes.

## Why `to_string_lossy()` exists

Sometimes you do want displayable text anyway. That is what `to_string_lossy()` is for.

```rust
let text = os_string.to_string_lossy();
```

This says:

> convert this OS string into something printable as UTF-8 text, replacing any invalid parts if needed

If the filename is already valid UTF-8, nothing special happens. If not, Rust substitutes replacement characters for the invalid pieces.

That is why it is called **lossy**: the conversion may not preserve the exact original bytes.

## Why it returns `Cow<str>`

`to_string_lossy()` returns a `Cow<str>`, not a plain `String`.

`Cow` means "clone on write." In practice here:

- if the filename was already valid UTF-8, Rust can borrow the existing text
- if invalid bytes had to be replaced, Rust allocates an owned string

So `Cow<str>` lets Rust avoid allocating when it does not need to.

This is a good example of Rust making efficiency visible in the type system.

## Why cawir calls `into_owned()`

In `list_files`, we do:

```rust
let mut name = entry.file_name().to_string_lossy().into_owned();
```

`into_owned()` turns the `Cow<str>` into a real owned `String`.

We need that because the next step mutates the string:

```rust
if file_type.is_dir() {
    name.push('/');
}
```

That `push('/')` requires an owned, mutable `String`.

So the chain is:

1. get the filename as `OsString`
2. convert it into displayable text with `to_string_lossy()`
3. turn that into an owned `String`
4. mutate it and return it

## Why lossy conversion is acceptable here

In cawir's current `list_files` tool, the output is for:

- the human reading the tool output
- Claude deciding what file to inspect next

So a display-oriented string is fine.

If we were building a path round-trip system where exact bytes mattered, converting early to `String` would be the wrong move. We would keep `OsString`, `PathBuf`, or `Path` longer instead.

That is the key tradeoff:

- use `String` when you need text
- use `OsString` or path types when you need exact filesystem fidelity

## Mental model from other languages

Many languages blur this distinction and treat filenames as ordinary strings.

Rust separates them on purpose:

- `String` is validated text
- `OsString` is OS-native path-like text data

That extra precision can feel verbose at first, but it prevents a lot of hidden assumptions around encoding and file handling.
