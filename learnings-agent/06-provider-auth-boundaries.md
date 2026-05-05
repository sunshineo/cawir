# Provider/auth boundaries

Provider and auth are related, but they are not the same boundary.

A provider boundary answers:

```text
What URL do we call?
What JSON do we send?
What JSON do we parse?
How do tool calls map into the shared session model?
```

An auth boundary answers:

```text
Where do credentials come from?
Which credential options are valid for this provider?
How does the chosen credential attach to the HTTP request?
```

Keeping these separate matters because providers can accept more than one credential option. OpenAI currently accepts both `ApiKey` and `CodexOAuth` in cawir. Both attach as bearer tokens, but they are different credential sources and have different product meaning.

## Provider-declared auth

Each provider declares accepted credential options:

```text
Anthropic -> ApiKey
OpenAI    -> ApiKey, CodexOAuth
Ollama    -> no credential option later
```

Startup and `/provider <name>` resolve credentials from that declaration. The agent loop receives an `ActiveCredential` and does not need to know whether it came from the credentials file, the environment, or `.env`.

Startup should not trap the user in setup for a guessed provider. The order is:

```text
CAWIR_PROVIDER if set -> saved preference -> providers with usable credentials -> provider prompt
```

Only after a provider is selected should cawir ask which credential option to acquire.

Re-authentication is explicit:

```text
/provider openai codex-oauth --reset
```

That command intentionally bypasses credential lookup and overwrites the saved credential for the selected provider/option.

The saved provider preference is deliberately not a credential store. It records:

```text
provider = openai
credential option = codex-oauth
```

The actual API key or OAuth token bundle lives in `credentials.json` under the OS config directory. This keeps "which route should startup try first?" separate from "what secret should be attached to a request?"

## Why this is useful

Without this split, adding a second OpenAI credential type would usually leak into the REPL and agent loop:

```text
if provider is OpenAI and auth is OAuth, use this header...
```

That kind of condition belongs at the auth/provider edge, not in orchestration. The core loop should keep saying: "send this history to the active provider with the active credential."

## Same provider, different route

Auth can influence the provider route without taking over the whole provider boundary.

For OpenAI:

```text
api-key      -> https://api.openai.com/v1/chat/completions
codex-oauth  -> https://chatgpt.com/backend-api/codex/responses
```

The important boundary rule is that this decision stays inside `openai.rs`. The REPL does not need to know that ChatGPT OAuth uses a different URL and a Responses-style payload. It only selects `provider = openai` and `credential option = codex-oauth`.

The ChatGPT Codex backend also requires a non-empty `instructions` field and `stream: true`. For now cawir sends a small fixed instruction string from `openai.rs`, requests a streaming response, and collects the SSE events internally before returning `ProviderResponse::Text` or `ProviderResponse::ToolUse`. That is provider-wire compatibility, not a full prompt-management or user-visible streaming system. Richer system/developer instructions can be extracted later when modes, config, and session persistence create concrete pressure; token-by-token display still belongs to the later streaming checkpoint.

The device-code OAuth flow returns ChatGPT OAuth tokens. It is not the same thing as an OpenAI API key, so cawir should not treat the returned `id_token` as a universal way to mint an API-key-style model token. Browser login flows can have extra account or organization context; the device-code token cawir gets may not.

## Current simplifications

The credentials file lookup falls through to environment lookup. This keeps local development and CI usable when no saved credential exists. Missing credentials are still reported clearly after all lookup sources are tried.

Codex OAuth acquisition uses the same device-code endpoints and public client id as the official OpenAI Codex CLI. The browser callback flow is more complex and is not needed for this checkpoint.
