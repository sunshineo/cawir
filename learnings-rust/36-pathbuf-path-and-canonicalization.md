# `PathBuf`, `Path`, and canonicalization

Checkpoint 8.5a added workspace path policy for file tools. That introduced a useful Rust distinction:

```text
PathBuf : owned path data
Path    : borrowed path view
```

This mirrors the more familiar text pair:

```text
String : owned text
str    : borrowed text view
```

## Why prepared tool inputs use `PathBuf`

The tool pipeline now has two phases:

```text
prepare input
apply policy / maybe ask approval
execute prepared input
```

Preparation resolves and validates the path. Execution may happen after approval, so the prepared value needs to carry the resolved path with it:

```rust
enum PreparedToolInput {
    ReadFile { path: PathBuf },
    WriteFile { path: PathBuf, content: String },
}
```

If this stored `&Path`, the prepared value would borrow a path created inside `prepare`, which would require lifetimes across the prepare / approve / execute boundary. Owning a `PathBuf` keeps the data model simple: once preparation succeeds, the prepared action owns everything execution needs.

Execution functions can still borrow:

```rust
fn execute_read_file(path: &Path) -> Result<String>
```

The enum owns the path. The executor borrows it for the duration of the filesystem call.

## Why `canonicalize()` is different for reads and writes

`canonicalize()` asks the OS for the real absolute path. It resolves:

```text
.
..
symlinks
```

But it only works when the path already exists.

For `read_file` and `list_files`, the target must already exist, so cawir can canonicalize the exact requested path:

```rust
let canonical_path = lexical_path.canonicalize()?;
```

For `write_file`, the target may be a new file. Canonicalizing the new filename would fail. The safer rule is:

```text
if the file exists:
    canonicalize the file and check it is inside the project

if the file does not exist:
    canonicalize the parent directory and check it is inside the project
    append the new file name
```

That lets cawir create new files without silently creating new directories.

## Prefix checks should be path checks

The workspace boundary check uses path structure:

```rust
canonical_path.starts_with(&project_root)
```

This is better than string prefix checks. Filesystem paths are not just strings; they have components, separators, roots, and platform-specific behavior.

Before canonicalization, cawir also does a lexical normalization pass over `.` and `..` components so obvious traversal attempts can be denied before touching the filesystem.

## Missing parent directories

`write_file` currently requires the parent directory to exist. Creating missing parents is a separate filesystem mutation and should be explicit later, for example:

```json
{
  "path": "src/new/nested/file.rs",
  "content": "...",
  "create_parents": true
}
```

That future shape would let the approval prompt name both effects: creating directories and writing the file.

## Passing `&Path` through the turn

Checkpoint 8.5i made the project root explicit in tool execution:

```rust
fn execute_tool_uses_with_approval(
    registry: &ToolRegistry,
    project_root: &Path,
    // ...
)
```

This uses `&Path` instead of `PathBuf` because the function does not need to own the root. The session/runtime already owns the project path for the turn; tool execution only needs to borrow it long enough to canonicalize and compare requested paths.

The distinction is:

```text
PathBuf: keep this path as part of my data
&Path: inspect or use this path during this call
```

Borrowing the root also makes the data flow visible. A function that accepts `&Path` is no longer secretly coupled to `std::env::current_dir()`, which makes tests and resumed-session behavior easier to reason about.
