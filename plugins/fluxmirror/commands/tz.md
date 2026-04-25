---
description: Alias for /fluxmirror:timezone
argument-hint: <KST|JST|PST|EST|UTC|Asia/Seoul|...>
---

Invoke the canonical skill via the Skill tool, forwarding `$ARGUMENTS`:

```
Skill(skill="fluxmirror:timezone", args="$ARGUMENTS")
```

Output its result verbatim — do NOT re-derive the behavior or
paraphrase. This guarantees `/fluxmirror:tz` and `/fluxmirror:timezone`
produce byte-identical output.
