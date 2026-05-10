# Edit tool ownership and string replacement

Checkpoint 8.5c added `edit_file`, which introduced a few Rust ideas around enum design, owned data, and standard string APIs.

## Permission enums can be categories

`edit_file` reuses `ToolKind::WriteFile` instead of adding a new `ToolKind::EditFile`.

That is intentional. `ToolKind` currently describes permission categories, not every concrete tool:

```rust
pub(crate) enum ToolKind {
    ReadOnly,
    WriteFile,
    Shell,
    ExitPlanMode,
}
```

From the policy layer's point of view, `write_file` and `edit_file` have the same risk: both mutate files. A separate enum variant is useful only once policy needs to distinguish them, such as allowing targeted edits while still asking before full rewrites.

## Prepared inputs own their strings

Tool preparation reads borrowed strings out of JSON:

```rust
let old_string = input_string(input, "edit_file", "old_string")?;
```

The return type is `&str`, borrowed from the `serde_json::Value`.

Prepared tool input stores owned `String`s:

```rust
EditFile {
    old_string: String,
    new_string: String,
}
```

That ownership matters because prepared input crosses the prepare -> approval -> execute boundary. If the enum stored `&str`, it would borrow from the temporary JSON input and require lifetimes across that whole flow. Owning `String` keeps the prepared action self-contained.

This is the same pattern as `PathBuf` vs `Path`:

```text
String  : owned text
&str    : borrowed text view
PathBuf : owned path
&Path   : borrowed path view
```

## Standard string APIs are enough for exact edits

For this checkpoint, cawir does exact text replacement, not patch parsing.

The needed operations map directly to standard library methods:

```rust
let matches = content.matches(old_string).count();
let updated_one = content.replacen(old_string, new_string, 1);
let updated_all = content.replace(old_string, new_string);
```

`matches(...).count()` validates whether the target is missing, unique, or ambiguous. `replacen(..., 1)` changes one unique occurrence. `replace(...)` changes every occurrence when `replace_all` is explicit.

This keeps the implementation small and testable. A patch parser would add line hunks, context matching, malformed patch errors, and partial-application rules before cawir needs them.

## Invalid tool requests are `ToolInput`

Missing and ambiguous matches are not Rust bugs. They are invalid requests from the model.

That makes `Error::ToolInput` the right error shape:

```rust
Error::ToolInput {
    tool: "edit_file".to_string(),
    message: "...".to_string(),
}
```

A panic would crash on a normal recoverable situation. A silent no-op would be worse, because the model might believe the edit happened. Returning a typed tool-input error lets the agent show the model what went wrong so it can re-read, provide more context, or set `replace_all`.
