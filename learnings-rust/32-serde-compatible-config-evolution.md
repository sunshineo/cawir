# Serde-compatible config evolution

Checkpoint 4e added model preferences to the saved provider preference:

```rust
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ProviderPreference {
    pub(crate) provider: String,
    pub(crate) auth_option: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) models: BTreeMap<String, String>,
}
```

Older config files only had:

```json
{
  "provider": "ollama",
  "auth_option": "none"
}
```

Without `#[serde(default)]`, deserializing that older file into the new struct would fail because `models` is missing. With `#[serde(default)]`, Serde fills the field with `BTreeMap::default()`, which is an empty map.

`skip_serializing_if = "BTreeMap::is_empty"` controls the opposite direction. If there are no saved model preferences yet, Serde omits the `models` field when writing JSON. That keeps the file small and avoids writing noisy empty state.

## Why the field is a map

The selected model is keyed by both provider and auth option:

```rust
format!("{provider}:{auth_option}")
```

That matters because the same provider can expose different model sets through different auth routes. For OpenAI, an API key and Codex OAuth are both "openai", but they do not necessarily have access to the same models.

## Test the migration shape

The important regression test is not only "can the new format save?" It is also:

```rust
let preference: ProviderPreference = serde_json::from_str(
    r#"{
        "provider": "ollama",
        "auth_option": "none"
    }"#,
)?;

assert!(preference.models.is_empty());
```

That test protects users who already have a config file on disk. Config changes should usually include this kind of backward-compatibility test before committing.
