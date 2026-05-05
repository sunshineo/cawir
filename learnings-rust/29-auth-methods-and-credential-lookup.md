# Auth options and credential lookup

Checkpoint 4c split provider wire format from credential attachment.

Before this step, the agent loop passed a raw `api_key: &str` into every provider. That worked while every provider used one API key shape, but it mixed two responsibilities:

- provider wire format: URL, JSON request shape, JSON response parsing
- credential option: where the credential comes from and how it attaches to HTTP

The new shape is:

```rust
enum AuthOption {
    ApiKey(ApiKeyCredential),
    CodexOAuth(CodexOAuthCredential),
}
```

This is a data-carrying enum. Read `AuthOption` as "one way this provider knows how to authenticate." Each variant can store the metadata needed for that option. For example, API key auth stores the environment variable name, the saved-credential key, and the request attachment style.

After lookup, cawir stores the selected runtime value as:

```rust
struct ActiveCredential {
    option_name: &'static str,
    request_auth: RequestAuth,
    secret: String,
    source: CredentialSource,
    chatgpt_account_id: Option<String>,
}
```

Read `ActiveCredential` as "the credential cawir actually resolved for this provider right now." That distinction matters:

- `AuthOption` is static provider metadata.
- `ActiveCredential` is runtime state and may contain a secret.

## Inherent methods on enum

The first 4c pass used a tiny `AuthMethod` trait for methods such as `name()` and `attach()`. That was more vocabulary than the code needed. The current code puts those methods directly on `AuthOption`:

```rust
impl AuthOption {
    fn name(&self) -> &'static str;
    fn env_var(&self) -> Option<&'static str>;
    fn attach(&self, request: reqwest::RequestBuilder, secret: Option<&str>)
        -> reqwest::RequestBuilder;
}
```

This is the simpler idiomatic Rust shape when the set of cases is known and local. A trait becomes useful later if unrelated types need to provide the same behavior behind a shared boundary.

In languages with nullable values, `env_var()` might return `null`. In Rust it returns `Option<&'static str>`:

- `Some("OPENAI_API_KEY")` means this auth option has an environment variable.
- `None` means it does not, as with no-auth local providers.

Callers must handle both cases with `match`, `if let`, or combinators.

## Current crate choices

`directories` computes OS-appropriate app directories. cawir uses it to find the config directory for:

- `provider.json`: selected provider and credential option
- `credentials.json`: saved API keys and OAuth token bundles

`rpassword` reads API keys from the terminal without echoing them back to the screen.

`base64` decodes JWT payloads so cawir can inspect token expiration and ChatGPT account metadata.

## Practical lookup behavior

The source lookup order is:

```text
credentials.json -> environment -> .env
```

`dotenvy` loads `.env` into process environment before credential resolution, without overriding real environment variables. That preserves the intended priority while keeping lookup code simple: after the credentials file, `std::env::var(...)` can see both real env vars and `.env` values.

Provider-declared option order still matters. If OpenAI lists `ApiKey` before `CodexOAuth`, then API-key credentials win unless the saved provider preference asks for `CodexOAuth` first.

## Acquisition and persistence

4c now does more than lookup. If no credential exists, cawir can acquire one:

- API key: prompt for the key without echoing it to the terminal, then save it in `credentials.json`.
- Codex OAuth: use OpenAI Codex's device-code flow, exchange the resulting authorization code for OAuth tokens, then save the token bundle in `credentials.json`.

The selected provider and credential option are saved in `provider.json` under the OS config directory from `directories`.

The Codex OAuth access token is used as the request bearer token. When cawir can read an `exp` claim from the token and it is expired or nearly expired, it refreshes using the saved refresh token and writes the new token bundle back to `credentials.json`.

This is also why the stored OAuth shape does not include a separate `model_token`. In Rust terms, removing that field from `CodexOAuthTokens` changes the data model and simplifies ownership: `active_oauth_credential(...)` can move `tokens.access_token` directly into `ActiveCredential.secret`. A move transfers the `String` without cloning its heap allocation. That is idiomatic when the caller no longer needs the original token bundle after constructing the active credential.

On Unix, cawir writes `credentials.json` with `0600` permissions: readable and writable by the current user only. This is not as strong as an OS credential store, but it avoids assuming macOS and keeps the storage model visible while learning.
