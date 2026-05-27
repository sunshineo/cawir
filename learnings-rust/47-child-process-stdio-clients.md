# Child process stdio clients

Checkpoint 14c made `cawir exec` start `cawir app-server` as a child process and
talk to it over piped stdin/stdout.

## `Command` configures a child process

Rust's standard process API starts with `std::process::Command`:

```rust
let mut child = Command::new(env::current_exe()?)
    .arg("app-server")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()?;
```

`Command::new(...)` is the builder. `spawn()` actually starts the process.

`Stdio::piped()` means the parent process wants a handle to that stream. Here
`exec` writes JSONL requests into the child app-server's stdin and reads JSONL
messages from the child app-server's stdout.

`Stdio::inherit()` means the child uses the parent's stream directly. In 14c the
child app-server's stderr is inherited so unexpected diagnostics still appear in
the terminal without corrupting stdout, which is reserved for protocol messages.

## Taking child streams moves the handles

After spawning, `child.stdin` and `child.stdout` are `Option` values:

```rust
let mut child_stdin = child.stdin.take().ok_or_else(...)?;
let child_stdout = child.stdout.take().ok_or_else(...)?;
```

The handles can only have one owner. Calling `.take()` moves the stream out of
the `Child` and leaves `None` behind. That matches Rust's ownership rule: two
parts of the program should not both think they exclusively own the same pipe.

## `BufReader` adds line-oriented reading

Raw stdout implements `Read`, but the app-server protocol is JSONL: one JSON
message per line. Wrapping stdout in `BufReader` gives access to `read_line`:

```rust
let mut reader = BufReader::new(child_stdout);
reader.read_line(&mut line)?;
```

This is why the exec client can process server messages one line at a time:
response, notification, server request, response, and so on.

## A stdio protocol client is still just typed data

The client writes typed request structs with `serde_json::to_writer`, then reads
typed response/request/notification enums with `serde_json::from_str`.

The process boundary is runtime behavior, but the possible message shapes are
still represented at compile time as Rust structs and enums. Invalid JSON or an
unexpected protocol shape becomes a `Result::Err`, not undefined behavior.
