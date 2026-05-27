# Ratatui, Crossterm, and terminal event loops

Checkpoint 14d adds the first terminal UI.

## Ratatui renders from state

Ratatui is an immediate-mode terminal UI library. The application owns normal
Rust data:

```rust
struct TuiState {
    transcript: Vec<String>,
    tools: Vec<String>,
    input: String,
    pending_approval: Option<PendingApproval>,
}
```

Each frame, the renderer draws widgets from that state:

```rust
terminal.draw(|frame| render_state(frame, &state))?;
```

The widgets are not long-lived objects that store app state. They are rebuilt
from the current state every frame. This fits Rust well because state ownership
stays in one struct and rendering borrows it immutably.

## Crossterm owns terminal mode

Crossterm provides the terminal control pieces Ratatui needs:

```rust
enable_raw_mode()?;
execute!(stdout, EnterAlternateScreen)?;
```

Raw mode means key presses are delivered directly instead of waiting for a full
line. The alternate screen lets the TUI draw a full-screen interface without
overwriting the normal shell scrollback.

Cleanup matters:

```rust
disable_raw_mode()?;
execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
terminal.show_cursor()?;
```

If a terminal program forgets cleanup, the shell can be left in a confusing
state where input is not echoed or the cursor is hidden.

## Background threads can bridge blocking protocol reads

The App Server stdout stream is blocking JSONL. The terminal also needs to poll
keyboard input. The MVP uses a background thread to read App Server messages and
send them into the UI loop through a channel:

```rust
let (sender, receiver) = mpsc::channel();

thread::spawn(move || {
    loop {
        let message = read_server_message(&mut reader)?;
        sender.send(message)?;
    }
});
```

The UI loop can then redraw, poll keys, and drain any protocol messages that
arrived since the previous frame. This avoids freezing the interface while the
model is streaming or while the server is waiting for an approval response.

## Text input needs explicit cursor state

A Ratatui `Paragraph` can display the input string, but it does not make that
string into an editor. If the app only stores this:

```rust
input: String,
```

then `push` and `pop` naturally edit only the end of the line. Moving left,
inserting in the middle, Backspace before the cursor, and Delete after the
cursor all require another piece of state:

```rust
input_cursor: usize,
```

In cawir this cursor is a byte index into the UTF-8 `String`. That is why cursor
movement uses character boundaries instead of blindly subtracting one from the
index. ASCII letters are one byte, but many Unicode characters are multiple
bytes, and Rust strings can only be sliced or edited at valid UTF-8 boundaries.

The visible terminal cursor is also explicit. Rendering the input text is not
enough; the frame must place the cursor:

```rust
frame.set_cursor_position(Position { x, y });
```

The terminal decides whether that cursor blinks. The TUI's job is to keep it at
the right cell for the current input state.
