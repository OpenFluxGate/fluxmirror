---
description: Summarize the last 7 days of AI agent activity from FluxMirror SQLite
argument-hint: [--html] [--out PATH] [--no-git-narrative]
---

Run the binary with the user's arguments and forward its output verbatim,
then add a 2-3 sentence business summary of what was shipped this week.
Base it on the "Shipped this week" commit-narrative section and the
heaviest activity clusters. Be concrete. Use the user's preferred language.

```bash
fluxmirror week $ARGUMENTS
```
