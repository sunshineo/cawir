# Model selection and capability discovery

Checkpoint 4e moved cawir from one hardcoded model per provider toward runtime model selection:

```text
/model
/model <name>
```

The key design point is that model availability is not just a provider property. It is a property of:

```text
provider + auth option + account or local runtime
```

Examples:

- `openai + api-key` can see the models available to that API project.
- `openai + codex-oauth` can see the models exposed by the ChatGPT Codex backend.
- `ollama + none` can see only the models installed on the local Ollama server.

Because of that, cawir saves model preferences with a provider/auth key such as:

```text
openai:api-key
openai:codex-oauth
ollama:none
```

This avoids a subtle bug where selecting a Codex OAuth model would accidentally become the remembered OpenAI API-key model.

## Dynamic lists first, fallbacks second

Hardcoded model lists get stale quickly and cannot reflect account-specific access. `/model` should query the active provider/auth route when possible:

```text
Anthropic API key -> provider model list endpoint
OpenAI API key    -> /v1/models, filtered to chat-capable names
OpenAI Codex OAuth -> ChatGPT Codex models endpoint
Ollama            -> /api/tags
```

Fallback models still matter. They give the REPL something useful to display if the model-list request fails, and they provide a startup default before any model has been selected.

The design separates three related ideas:

- default model: provider-auth route's preferred starting point
- selected model: user's saved choice for the current provider/auth route
- available models: dynamically discovered list for display and validation

## Testing implication

A normal live smoke test proves that one default request can work. It does not prove that model listing works.

The added `live_models_and_switch` test covers a different path:

```text
resolve provider/auth credential
query available models
select the default model if listed, otherwise the first listed model
send one request with that selected model
```

This does not need to try every listed model. The useful coverage is that the list endpoint parses correctly and at least one discovered model can be passed back into the send path.
