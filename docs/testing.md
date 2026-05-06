# Testing

cawir has two test layers.

## Offline tests

Run these constantly:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

These tests do not require provider credentials or network access. They should be fast and deterministic.

## Live provider tests

Live tests are ignored by default because they use real credentials, real network calls, and provider quota.

Run the live smoke test when changing provider wire format, auth resolution, credential refresh, or request/response parsing. Select the route with `PROVIDER` and `AUTH_OPTION`.
The smoke test uses the provider's default model for the selected auth option.
Run `live_models_and_switch` when changing model-listing or model-selection behavior. It queries the provider's model list, selects one returned model, and sends one request through that selected model.

Anthropic API key:

```sh
PROVIDER=anthropic AUTH_OPTION=api-key cargo test live_smoke -- --ignored --nocapture
PROVIDER=anthropic AUTH_OPTION=api-key cargo test live_models_and_switch -- --ignored --nocapture
```

OpenAI API key:

```sh
PROVIDER=openai AUTH_OPTION=api-key cargo test live_smoke -- --ignored --nocapture
PROVIDER=openai AUTH_OPTION=api-key cargo test live_models_and_switch -- --ignored --nocapture
```

OpenAI Codex OAuth:

```sh
PROVIDER=openai AUTH_OPTION=codex-oauth cargo test live_smoke -- --ignored --nocapture
PROVIDER=openai AUTH_OPTION=codex-oauth cargo test live_models_and_switch -- --ignored --nocapture
```

Ollama local no-auth:

```sh
PROVIDER=ollama AUTH_OPTION=none cargo test live_smoke -- --ignored --nocapture
PROVIDER=ollama AUTH_OPTION=none cargo test live_models_and_switch -- --ignored --nocapture
```

Credential lookup is the same chain as the REPL:

```text
credentials.json -> environment -> .env
```

For Codex OAuth, authenticate once through `/provider openai codex-oauth --reset`; future live tests can reuse and refresh the saved token bundle.

For Ollama, start the local Ollama app or service first and make sure the configured model is available:

```sh
ollama pull qwen3:8b
```
