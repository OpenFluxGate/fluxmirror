---
description: Alias for /fluxmirror:setup (shows current config when no args)
argument-hint: [language <value>] [timezone <value>]
---

Invoke the canonical skill via the Skill tool, forwarding `$ARGUMENTS`:

```
Skill(skill="fluxmirror:setup", args="$ARGUMENTS")
```

Output its result verbatim — do NOT re-derive the behavior or
paraphrase. This guarantees `/fluxmirror:cfg` and `/fluxmirror:setup`
produce byte-identical output.
