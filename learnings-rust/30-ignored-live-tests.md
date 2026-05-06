# Ignored live tests

Rust tests can be marked with `#[ignore]`:

```rust
#[tokio::test]
#[ignore = "uses real provider credentials and network"]
async fn live_smoke() -> Result<()> {
    // ...
}
```

Ignored tests compile with normal tests, but they do not run during plain `cargo test`.

Run ignored tests explicitly:

```sh
PROVIDER=openai AUTH_OPTION=codex-oauth cargo test live_smoke -- --ignored --nocapture
```

The first part is Cargo's filter: `live_smoke`.

The part after `--` is passed to Rust's test harness:

- `--ignored` means run ignored tests.
- `--nocapture` means show `println!` and `eprintln!` output.

For cawir, the live smoke test also requires `PROVIDER` and `AUTH_OPTION`. This keeps one stable command shape as the provider/auth matrix grows:

```sh
PROVIDER=anthropic AUTH_OPTION=api-key cargo test live_smoke -- --ignored --nocapture
PROVIDER=openai AUTH_OPTION=api-key cargo test live_smoke -- --ignored --nocapture
PROVIDER=openai AUTH_OPTION=codex-oauth cargo test live_smoke -- --ignored --nocapture
```

## Unit tests can still be live tests

Rust has integration tests in `tests/*.rs`, but a live test does not have to live there. cawir keeps this smoke test inside `src/provider.rs` for now.

That is useful because the provider and auth modules are still mostly `pub(crate)` or private. A test module inside the same file can use those private helpers directly:

```rust
#[cfg(test)]
mod tests {
    use super::*;
}
```

This avoids making internal provider/auth details public just to test them. Later, if cawir grows a stable runtime API, live tests can move to `tests/*.rs` and exercise that public surface instead.
