# Loop forms and agent loops

Rust has three main loop forms:

```rust
loop { ... }
while condition { ... }
for item in iterator { ... }
```

There is also `while let`, which loops while a pattern matches:

```rust
while let Some(item) = iterator.next() {
    println!("{item}");
}
```

## `loop`

`loop` means "repeat forever until something inside exits."

That fits the agent loop because the stop condition is not known before the iteration starts. cawir has to call Claude first, then decide what happened:

```rust
loop {
    match ask_claude(client, api_key, history).await? {
        ClaudeResponse::Text(reply) => {
            println!("claude: {}", reply);
            return Ok(());
        }
        ClaudeResponse::ToolUse(blocks) => {
            let tool_results = execute_tool_uses(&blocks)?;
            history.push(Message::assistant(blocks));
            history.push(Message::user_tool_results(tool_results));
        }
    }
}
```

`return Ok(())` exits the function, which also exits the loop. The `ToolUse` branch does not return, so the next loop iteration calls Claude again with the new tool results in history.

## `while`

`while` repeats while a condition is true:

```rust
while attempts < 3 {
    attempts += 1;
}
```

This is best when the condition can be checked before each iteration. It is less natural for the agent loop because the important condition is inside the response from Claude.

## `for`

`for` iterates over a finite collection or iterator:

```rust
for block in blocks {
    match block {
        MessageContent::Text { text } => println!("claude: {}", text),
        MessageContent::ToolUse { id, name, input } => {
            // execute one tool call
        }
        MessageContent::ToolResult { .. } => {}
    }
}
```

This is the right shape for content blocks because the response already contains a finite list of blocks.

## Loop caps

An open-ended agent loop can burn tokens if the model repeatedly asks for tools and never returns a final text answer. cawir protects the current read-only loop with:

```rust
const MAX_TOOL_ROUNDS: usize = 42;
```

Each Claude response containing tool use counts as one tool round. If the loop exceeds the cap, cawir returns a typed error instead of continuing indefinitely:

```rust
Error::ToolLoopLimitExceeded(MAX_TOOL_ROUNDS)
```

This is still a normal recoverable error. The REPL prints it, rolls back the current turn's history, and waits for the next user prompt.
