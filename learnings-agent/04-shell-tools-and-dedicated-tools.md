# Shell tools and dedicated tools

Checkpoint 3i added `shell`, the broadest tool so far.

Shell is powerful because it can run tests, formatters, builds, git commands, search commands, and small one-off scripts. It is also risky because it can imitate narrower tools:

- `ls` can replace `list_files`
- `cat` can replace `read_file`
- `printf ... > file` can replace `write_file`

For cawir, the narrower tools should usually win. They have clearer intent, smaller inputs, easier approval behavior, and simpler future policy rules.

## Prompt/schema steering

The `shell` tool description now explicitly says:

```text
Do not use shell for directory listings, file reads, or file writes; use the dedicated list_files, read_file, and write_file tools for those.
```

This is a nudge, not a guarantee. The model can still request shell commands that perform file operations. Checkpoint 3 keeps this as prompt-level behavior because the real policy system starts in Checkpoint 4.

## Nonzero exit is useful data

If a shell command exits nonzero, that should usually go back to the model as a normal tool result containing exit status, stdout, and stderr.

Example: failed tests are not a cawir failure. They are information the model can use to fix the code.

Tool execution errors are different:

- cawir could not start the process
- approval was denied
- the tool input was malformed

Those become error tool results.

## Output budgets

Shell can produce huge output. Treating stdout and stderr as one string is acceptable for the first working tool, but future checkpoints should add output caps and truncation markers.

This belongs with the broader tool-output budgeting work: read tools, shell tools, and future search tools can all inject large content into history.
