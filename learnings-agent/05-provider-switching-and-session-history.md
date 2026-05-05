# Provider switching and session history

Changing model providers is not the same thing as starting a new conversation.

In cawir, the REPL owns three pieces of runtime state that matter here:

```text
provider
api_key
history
```

The `/provider <name>` command changes the active `provider` and matching `api_key`. It does not change `history`.

That means this sequence:

```text
cawir> hi, what model am i talking to?
anthropic: I'm Claude...
cawir> /provider openai
provider: openai
cawir> hi, what model am i talking to now?
```

sends the earlier Anthropic-backed conversation history to OpenAI on the next model call. The OpenAI model can see the previous assistant message saying "I'm Claude", and may continue that identity because model self-identification is usually inferred from context, not from reliable runtime metadata.

## Why this happens

The internal `Message` type is provider-neutral. Each provider adapter translates that shared session history into its own wire format when `send` is called.

That design is useful because the agent loop can call one provider boundary instead of owning Anthropic-specific or OpenAI-specific message formats. But it also means provider switching is semantically a continuation unless the agent explicitly starts a new session.

## Design options

Keep history on provider switch:

- Preserves the user's working context.
- Makes switching providers cheap and unsurprising from a data-loss perspective.
- Can create confusing model identity and behavior because the new provider sees old provider outputs.

Clear history on provider switch:

- Avoids cross-provider persona/context leakage.
- Silently discards useful work unless the user asked for a fresh session.
- Is especially risky once tool results, file reads, and planning context are stored in history.

Start a new named session on provider switch:

- Makes the semantics clean.
- Requires session management that cawir does not have yet.
- Belongs later, near resume/session work or slash-command expansion.

For the current checkpoint, the best small behavior is: keep history, but warn clearly when switching providers.

```text
provider: openai
note: existing conversation history will be sent to openai on the next turn
```

A future `/new` or `/clear` command can give the user an explicit way to start fresh without tying that behavior to provider switching.

## Agent-design lesson

The model does not know which provider, account, binary, or runtime selected it unless the agent tells it through context or protocol metadata. Asking "what model are you?" is a weak test of provider selection.

For debugging provider routing, trust local instrumentation: the selected provider enum, request URL, auth method, and model field. Treat model self-description as generated text influenced by prior messages.
