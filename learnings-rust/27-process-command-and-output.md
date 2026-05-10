# Process commands and output

Checkpoint 3i added the first shell tool using Rust's standard process API:

```rust
let output = Command::new("sh").arg("-c").arg(command).output()?;
```

## `Command`

`std::process::Command` builds and runs child processes.

This form:

```rust
Command::new("cargo").arg("test").output()?
```

runs one executable directly with one argument.

This form:

```rust
Command::new("sh").arg("-c").arg(command).output()?
```

runs the system shell and asks it to interpret a full command string. That matches terminal-like UX because the command can include quoting, pipes, redirects, environment variables, `&&`, and other shell features.

The tradeoff is that the shell is a command language, not just a program launcher. That makes approval more important.

## `?` only means the process failed to start

`output()?` returns a Rust error if cawir could not start or wait for the process.

Examples:

- `sh` cannot be found
- the process cannot be spawned
- the operating system returns an IO error

A nonzero exit code is different. If `cargo test` runs and tests fail, the process still ran successfully from Rust's point of view. The exit code is data inside the output.

## stdout and stderr are bytes

Rust's `String` must always be valid UTF-8, but operating systems do not promise that process output is valid UTF-8. For that reason, `std::process::Output` stores:

```rust
stdout: Vec<u8>
stderr: Vec<u8>
```

cawir currently decodes those bytes with:

```rust
String::from_utf8_lossy(&output.stdout)
```

`from_utf8_lossy` keeps valid UTF-8 readable and replaces invalid bytes with the replacement character. This is pragmatic for a learning REPL because the tool result usually needs to be readable text for the model.

## `spawn` and `try_wait` for timeouts

`Command::output()` is convenient, but it waits until the process exits:

```rust
let output = Command::new("sh").arg("-c").arg(command).output()?;
```

That means there is no place to check whether the process has run too long. For a shell timeout, cawir needs the lower-level shape:

```rust
let mut child = Command::new("sh").arg("-c").arg(command).spawn()?;

loop {
    if child.try_wait()?.is_some() {
        let output = child.wait_with_output()?;
        break;
    }

    // check timeout, maybe kill, then sleep briefly
}
```

`spawn()` starts the process and returns a `Child` handle immediately. `try_wait()` checks whether the child has exited without blocking forever. That makes timeout enforcement possible.

After a timeout, `wait_with_output()` still matters: it reaps the child process and collects whatever stdout/stderr was produced before termination.

## Process groups on Unix

The shell tool runs commands through:

```sh
sh -c "<command>"
```

The direct child of cawir is often just `sh`; the actual work may be a grandchild process:

```text
cawir
└── sh
    └── sleep
```

If cawir kills only `sh`, the grandchild may keep running. On Unix, starting the shell in a new process group lets cawir signal the whole command group on timeout:

```rust
use std::os::unix::process::CommandExt;

command.process_group(0);
```

Then a signal sent to `-child.id()` targets the process group instead of one process. cawir still keeps a direct `child.kill()` fallback because process-group cleanup is Unix-specific and can fail.

## Bytes, UTF-8, and truncation boundaries

Output budgets are naturally byte budgets: files, process output, memory, and model-context payloads are all ultimately measured in bytes. But Rust `String` values must be valid UTF-8.

That means a tool cannot safely cut a string at an arbitrary byte offset. Some characters take multiple bytes:

```text
a  = 1 byte
é  = 2 bytes
世 = 3 bytes
```

If a byte cap lands in the middle of a multi-byte character, that prefix is not valid UTF-8. A strict text tool such as `read_file` should back up to the last valid UTF-8 boundary before creating a `String`.

This has nothing to do with English vs non-English text. Chinese comments are valid as long as the file is encoded as UTF-8:

```rust
// 这是一个中文注释
```

The invalid case is a file or output stream whose bytes are not UTF-8 at all, such as binary data or a legacy encoding.

## Strict vs lossy decoding

cawir uses two different decoding policies:

- `read_file` is strict because the tool contract says it reads a UTF-8 text file.
- `shell` is lossy because process output is arbitrary bytes and the model usually needs a readable preview.

Strict decoding protects the `read_file` contract. If cawir silently replaced invalid bytes in source files, the model might reason over text that is not actually in the file.

Lossy decoding is pragmatic for stdout/stderr. Command output may contain invalid bytes, terminal control sequences, or binary fragments, and a best-effort readable result is usually more useful than failing the whole shell tool.

## Checkpoint 8.5b update

Checkpoint 8.5b replaced the "one big string" shell result with bounded output:

- stdout and stderr are capped separately
- truncation markers are visible in the tool result
- long-running commands are killed after a timeout
- small outputs keep the same formatting as before

That keeps shell output from flooding conversation history.
