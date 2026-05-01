# `collect`, `Result`, and `FromIterator`

Checkpoint 3d introduced a very Rust-shaped pattern in `list_files`:

```rust
let mut entries = std::fs::read_dir(path)?
    .map(|entry| -> Result<String> {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let mut name = entry.file_name().to_string_lossy().into_owned();

        if file_type.is_dir() {
            name.push('/');
        }

        Ok(name)
    })
    .collect::<Result<Vec<_>>>()?;
```

This one expression teaches three related ideas:

1. `read_dir` is lazy, so iterator items can fail later.
2. `collect` can build a `Result<Vec<T>, E>` from iterator items of type `Result<T, E>`.
3. That behavior comes from the `FromIterator` trait.

## Why `read_dir` yields `Result<DirEntry, io::Error>`

`std::fs::read_dir(path)?` only means:

> Rust successfully opened the directory.

It does **not** mean Rust has already read every entry inside it.

Directory reading is lazy. As the iterator pulls entries from the OS, any individual step can still fail. So the iterator item type is:

```rust
Result<DirEntry, std::io::Error>
```

That is why the closure starts with:

```rust
let entry = entry?;
```

Each item coming in might already be an error.

## Why the closure returns `Result<String>`

Inside the closure, more than one thing can fail:

```rust
let entry = entry?;
let file_type = entry.file_type()?;
```

So the closure cannot honestly return just `String`. It has to return:

```rust
Result<String, Error>
```

In cawir, `Result<T>` is our project alias for `std::result::Result<T, Error>`.

That means each mapped item is one of:

- `Ok(String)` for a successfully formatted entry name
- `Err(Error)` if reading or inspecting that entry failed

## What `collect::<Result<Vec<_>>>()` does

This is the interesting part:

```rust
.collect::<Result<Vec<_>>>()?
```

If an iterator yields items of type:

```rust
Result<T, E>
```

then Rust knows how to collect those into:

```rust
Result<Vec<T>, E>
```

The behavior is:

- keep gathering `Ok(T)` values
- stop at the first `Err(E)`
- return that `Err(E)` immediately

So for:

```rust
Ok("a"), Ok("b"), Err("bad"), Ok("c")
```

collecting into `Result<Vec<_>, _>` returns:

```rust
Err("bad")
```

Two subtle points:

- You do **not** get the partial `Vec` back.
- Rust stops pulling items after the first error, so `"c"` is never requested.

This is the "all values or first error" pattern.

## `collect` is powered by `FromIterator`

`collect()` is not magic syntax for `Vec` specifically. It works for any type that implements the `FromIterator` trait.

Conceptually:

```rust
iterator.collect::<SomeType>()
```

means:

```rust
SomeType::from_iter(iterator)
```

A simplified version of the trait looks like:

```rust
trait FromIterator<A>: Sized {
    fn from_iter<T: IntoIterator<Item = A>>(iter: T) -> Self;
}
```

Read that as:

> "If you give me iterator items of type `A`, I know how to build `Self`."

Examples:

- `Vec<T>` implements `FromIterator<T>`
- `String` implements `FromIterator<char>`
- `Result<Vec<T>, E>` implements `FromIterator<Result<T, E>>`

That last impl is what powers cawir's `collect::<Result<Vec<_>>>()?` line.

## Why this pattern is idiomatic

In many languages, you would write a manual loop:

```text
create empty list
for each item:
  try to transform it
  if error: return error
  else push into list
return list
```

Rust lets you express that same control flow declaratively:

```rust
iter.map(...).collect::<Result<Vec<_>>>()?
```

It is concise, but still honest about failure:

- the iterator items may fail
- the transformation may fail
- the whole collection may fail

The types carry that information all the way through.

## Mental model

- `Iterator` says how values come out one by one.
- `FromIterator` says how to consume those values into a final type.
- `collect()` connects the two.

When the item type is `Result<T, E>`, collecting into `Result<Vec<T>, E>` means:

> build the whole vector if everything succeeds, otherwise return the first error.
