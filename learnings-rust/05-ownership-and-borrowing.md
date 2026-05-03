# Ownership and borrowing — `mut`, `&`, `&mut`

The single hardest Rust concept, and the foundation for everything else. Three rules layered on top of each other.

## Rule 1: Bindings are immutable by default

Opposite of most languages.

| TypeScript | Rust |
|---|---|
| `const x = 5` | `let x = 5;` — cannot reassign, cannot mutate |
| `let x = 5` | `let mut x = 5;` — can reassign, can mutate |

JS and Python default to mutable; Rust defaults to immutable. If you want to change a variable, you opt in explicitly with `let mut`.

## Rule 2: References are either shared or exclusive

A reference is a "borrowed view" of data — like a pointer, but tracked by the compiler.

| Syntax | Meaning | How many can coexist |
|---|---|---|
| `&T` | **Shared (immutable) reference.** Read-only view. | Many at once |
| `&mut T` | **Exclusive (mutable) reference.** Read-write view. | **One** at a time, and no shared refs alongside |

## Rule 3: Many readers OR one writer — never both

At any moment, each value has either:

- **Zero or more `&T` shared references** (readers), OR
- **Exactly one `&mut T` exclusive reference** (writer), with no shared refs alongside.

Never both. This is Rust's core data-race-prevention rule, enforced by the "borrow checker" at compile time.

TypeScript, Python, Java don't have this. You can mutate anything through any reference at any time (unless marked `readonly`, which is a promise, not a check). Rust forces the guarantee.

## How it shows up in `src/main.rs`

```rust
let mut line = String::new();
io::stdin().read_line(&mut line)?;
```

Two things must be true:

1. `let mut line` — the variable is declared mutable. Only a mutable binding can be borrowed mutably.
2. `&mut line` — lending a mutable reference to `read_line`, because it needs to append bytes into the String.

`read_line`'s signature (simplified):

```rust
fn read_line(&self, buf: &mut String) -> io::Result<usize>
```

It requires a `&mut String`. Passing `&line` (shared ref) would be a compile error — `read_line` can't append to a read-only view.

## Project example: the agent loop borrows history

In Checkpoint 3.5c, `lib.rs` still owns the conversation history:

```rust
let mut history: Vec<Message> = Vec::new();
```

For each user turn, the REPL lends that history to the agent loop:

```rust
agent::run_turn(&client, &api_key, &mut history).await
```

The function signature says exactly what the agent loop is allowed to do:

```rust
pub(crate) async fn run_turn(
    client: &reqwest::Client,
    api_key: &str,
    history: &mut Vec<Message>,
) -> Result<()>
```

`history: &mut Vec<Message>` means: "borrow this vector exclusively, let me modify it, but give it back when I am done." That lets `run_turn` push assistant replies and tool-result messages into the same `Vec<Message>` without taking ownership of it.

The alternatives would mean different things:

| Parameter | Meaning | Why not here |
|---|---|---|
| `history: Vec<Message>` | Take ownership of the whole vector. | `lib.rs` would lose `history` unless `run_turn` returned it. |
| `history: &[Message]` | Borrow a read-only slice. | The agent loop could read history but could not append new messages. |
| `history: &mut [Message]` | Mutably borrow a fixed-length slice. | The agent loop could edit existing messages but could not `push`. |
| `history: &mut Vec<Message>` | Mutably borrow the growable vector. | Correct: the caller keeps ownership, and the loop can append. |

While `run_turn` has `&mut history`, no other code can use `history`. After `.await` returns, the borrow is over, so `lib.rs` can safely regain control and roll back a failed turn:

```rust
history.truncate(history_len_before_turn);
```

That is the ownership model matching the design: the REPL owns session history, the agent loop gets temporary exclusive editing rights for one turn, then the REPL regains control.

## When you need `mut`

| Situation | `mut` needed? |
|---|---|
| `let x = 5;` then `x = 7;` | Yes (reassignment) |
| `let v = vec![1,2,3];` then `v.push(4);` | Yes (mutation) |
| `let v = vec![1,2,3]; for x in &v { ... }` | No (only borrowing shared) |
| `for x in &mut v { *x += 1; }` | Yes (mutable iteration) |
| Passing to a function that takes `&mut T` | Yes |
| Passing to a function that takes `&T` | No |

## Why Rust chose this

Immutable-by-default + the exclusive-writer rule gives Rust two guarantees most languages don't have:

- **No data races in safe code.** Ever. Can't have two threads mutating the same data without an explicit synchronization primitive.
- **Aliasing optimization.** The compiler can optimize assuming `&mut T` is really exclusive — safely, because the borrow checker enforces it.

The cost is the "borrow checker" yelling at you while the model clicks.

## Common beginner confusions

- **"Why doesn't `x = 5` work when `x` was declared `let x = 3`?"** Because bindings are immutable by default. Use `let mut x`.
- **"Why does this function want `&mut` instead of ownership?"** Because it wants to modify the value without consuming it. Ownership (moving the value in) would leave the caller without it afterward.
- **"Why can't I have two `&mut` references?"** The exclusive-writer rule. Allowing two would permit data races.

## The borrow checker

Rust's compile-time enforcer of rules 2 and 3. It analyzes your code to prove that at no point do you have two mutable references to the same value, or a mutable + shared reference coexisting. If it can't prove safety, it refuses to compile.

Feels punitive at first; becomes invisible once the mental model clicks.
