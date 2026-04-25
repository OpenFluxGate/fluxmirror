---
description: Alias for /fluxmirror:language
argument-hint: <korean|english|japanese|chinese>
---

Invoke the canonical skill via the Skill tool, forwarding `$ARGUMENTS`:

```
Skill(skill="fluxmirror:language", args="$ARGUMENTS")
```

Output its result verbatim — do NOT re-derive the behavior or
paraphrase. This guarantees `/fluxmirror:lang` and `/fluxmirror:language`
produce byte-identical output.
