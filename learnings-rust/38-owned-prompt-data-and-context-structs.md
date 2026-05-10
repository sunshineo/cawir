# Owned prompt data and context structs

## `String` vs `&str` in assembled data

`&str` is a borrowed view into text owned somewhere else. It is ideal for string literals or short-lived function inputs, but it needs a valid owner to outlive the borrow.

`String` owns its text allocation. That is usually simpler for assembled runtime data, such as a system prompt built from formatted environment text and file contents read from disk.

In 8.5d, `SystemPrompt` owns `String`s because some sections are created at runtime:

- `environment` formats the current path.
- `project_guidance` reads `AGENTS.md` / `CLAUDE.md` contents from disk.
- tests build prompt values directly.

If `SystemPrompt` stored `&str`, the prompt would need lifetime parameters tying it to temporary strings and file buffers. Owning the prompt avoids that complexity: build it, pass `&SystemPrompt` to providers, then drop it after the request.

## `Path::ancestors()` and `canonicalize()`

`Path::ancestors()` walks from a path upward toward the filesystem root. Reversing that list gives broad-to-specific loading: parent instructions first, child/project instructions later.

`canonicalize()` returns an absolute normalized path and resolves symlinks. That is useful for deduping because two different path spellings, such as `AGENTS.md` and a symlinked `CLAUDE.md`, can point to the same real file.

The pattern used in prompt memory is:

1. Canonicalize the project path.
2. Walk ancestors.
3. Reverse them.
4. For each `AGENTS.md` / `CLAUDE.md`, canonicalize the file path.
5. Store canonical file paths in a set so the same real file is loaded once.

## Context structs for growing function boundaries

Adding `project_root` to `run_turn` pushed the function over clippy's `too_many_arguments` lint. A context struct is the idiomatic fix when several arguments naturally travel together.

`TurnContext` groups per-turn dependencies:

- provider
- HTTP client
- credential
- model
- project root
- permission mode

The struct holds references, so passing it does not clone the client, provider, or credential. It also makes future additions easier to read because the call site names the fields.
