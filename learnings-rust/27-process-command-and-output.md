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

## Future work

Treating process output as one big string is good enough for Checkpoint 3i, but it is not a complete agent design.

Future work should add:

- byte or line caps for stdout and stderr
- clear truncation markers
- maybe separate structured fields for status, stdout, and stderr instead of one formatted string

That keeps shell output from flooding conversation history.
