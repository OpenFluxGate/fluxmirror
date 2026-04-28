---
description: Single-agent filtered report. Usage: <claude-code|qwen-code|gemini-cli> [--period today|yesterday|week] [--html]
argument-hint: <claude-code|qwen-code|gemini-cli> [--period today|yesterday|week] [--html] [--out PATH]
---

Run the binary with the user's arguments and forward its output verbatim,
then add a 2-3 sentence summary of what this agent's work looked like.
Be concrete. Use the user's preferred language.

```bash
fluxmirror agent $ARGUMENTS
```
