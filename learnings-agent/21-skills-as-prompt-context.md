# Skills As Prompt Context

## Summary

Skills are a prompt-context mechanism with progressive disclosure: discover cheap metadata early, activate from the user prompt, and insert full instructions only for matched skills. This keeps inactive workflow guidance out of model context while still making active guidance visible in request events and debug output.

Skills are reusable instruction bundles, not tools. A tool gives the model a callable capability; a skill changes how the model should work on a prompt. That puts skills in prompt assembly, not in `ToolRegistry`.

Checkpoint 13 keeps activation simple and inspectable:

- Skill packages live in configured skill directories or plugin `skills/` folders.
- Each skill has metadata (name, description, trigger guidance phrases) plus an instruction body.
- The runtime catalog stores metadata and a path, not every instruction body.
- A user prompt activates a skill by naming it, such as `$rust-tutor`, or by matching a configured trigger phrase.
- Only activated skills have their instruction bodies read and inserted into prompt assembly.
- The active skill set is chosen once at the start of the user turn and reused through the tool loop so follow-up tool-result requests keep the same workflow guidance.

Active skills are dynamic prompt context. They are intentionally visible in model-request events and REPL observability output, because changing active skills changes the system prompt and may affect provider prompt-cache reuse.
