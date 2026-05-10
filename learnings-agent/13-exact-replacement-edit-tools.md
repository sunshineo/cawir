# Exact replacement edit tools

Checkpoint 8.5c added `edit_file`, a targeted editing primitive beside `write_file`.

The agent-design lesson is that routine code edits should not require replacing entire files. A full-file write can accidentally drop unrelated user changes or stale content. An exact replacement tool makes the model identify the current text it intends to change.

## Shape of the tool

The useful minimal shape is:

```json
{
  "path": "src/tools.rs",
  "old_string": "exact current text",
  "new_string": "replacement text",
  "replace_all": false
}
```

`write_file` remains useful for new files and deliberate full rewrites. `edit_file` is for changing existing text in existing files.

## Ambiguity is a safety feature

If `old_string` appears more than once and `replace_all` is false, the edit should fail.

To edit one particular occurrence, the model must include enough surrounding context to make the target unique:

```text
ambiguous:
  "return Ok(())"

unique:
  "run_step()?;\nreturn Ok(())"
```

This forces the model to anchor the edit in local file structure instead of guessing which repeated string was intended.

## `replace_all` must be explicit

`replace_all` is useful for repeated mechanical changes, such as renaming a variable within one file.

It should not be the default. Replacing every occurrence is a broader effect than replacing one occurrence, so the model should ask for that effect explicitly in the tool input and the approval flow should show that a file mutation is happening.

## Tool errors help the model recover

These edit failures are recoverable:

```text
old_string not found
old_string matched multiple places
old_string equals new_string
path outside workspace
```

Returning them as tool errors is better than aborting the turn. The model can recover by reading the file again, using a more specific `old_string`, or choosing `replace_all` when every match should change.

The general principle: editing tools should fail closed when they cannot prove the edit target is the one intended.
