# Runtime Context Catalogs

Checkpoint 13 added `SkillCatalog`, which is a runtime catalog rather than durable session data. The catalog is rebuilt from settings and plugin folders for the active project, while `Session` stays focused on serializable conversation history and provider/mode metadata.

The Rust shape is deliberately owned, but split into metadata and active instructions:

- `SkillMetadata` owns the discovery fields: name, description, and trigger guidance.
- `SkillCatalog` owns a sorted `Vec<SkillEntry>`, where each entry carries metadata plus the manifest path. It does not store the instruction body.
- `Skill` is the active prompt-context value. It is built only after activation by reading the manifest's `instructions` field from disk.
- Activation uses a temporary `BTreeSet<String>` of prompt tokens. `BTreeSet` gives deterministic behavior while answering the simple question "does this exact skill name appear as a token?"

This is progressive disclosure in Rust data terms: metadata is cheap and always available to the host program, while large instructions are not deserialized, validated, stored, or sent to model context until the current user prompt actually needs them. Because the current format keeps metadata and instructions in one JSON file, discovery still opens the manifest; a future Markdown/frontmatter format could avoid reading body bytes during discovery too. A trait would still add indirection before there is more than one implementation.
