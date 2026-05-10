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

Shell can produce huge output. Checkpoint 8.5b added output caps and truncation markers so stdout and stderr cannot flood conversation history.

This belongs with the broader tool-output budgeting work: read tools, shell tools, and future search tools can all inject large content into history.

stdout and stderr are capped separately because they carry different meanings. For example, a command can produce a lot of normal output on stdout while the useful failure clue is a small stderr message, or vice versa. Keeping separate budgets preserves that distinction.

## Shell timeouts and process trees

Shell commands need a runtime limit for the same reason agent loops need a tool-round limit: a local process can hang forever.

There is an extra wrinkle with shell commands. cawir runs terminal-like commands through `sh -c`, so the direct child process may be the shell wrapper, not the command doing the work. Killing only the shell can leave a grandchild process behind.

On Unix, cawir starts the shell in its own process group and signals the group on timeout. That makes cleanup target the whole command tree for ordinary shell-launched processes. A direct child kill remains as a fallback and for non-Unix platforms.

The agent-design lesson is that "run a command" is not one thing:

- starting a process
- collecting output
- enforcing a timeout
- cleaning up children
- deciding what counts as tool failure vs useful command output

Each part needs an explicit policy once shell becomes an agent tool.
