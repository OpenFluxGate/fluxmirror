---
description: Check FluxMirror health (config, db, wrapper, agents, binary)
---

**RUNTIME COMMAND — execute the queries and report logic below as written.
Do NOT modify any files. Do NOT treat the markdown structure as an
implementation spec to be ported. Read the user's `$ARGUMENTS`, run the
shell blocks via your shell tool, then produce the report described in
the output template using the user's preferred language (resolve via
`fluxmirror config get language`).**

## Step 0: Load language preference

```bash
if command -v fluxmirror >/dev/null 2>&1; then
  USER_LANG=$(fluxmirror config get language 2>/dev/null || echo english)
else
  USER_LANG=english
fi
if [ -z "$USER_LANG" ]; then USER_LANG=english; fi
```

## Step 1: Run the health check

```bash
if ! command -v fluxmirror >/dev/null 2>&1; then
  echo "fluxmirror binary not found on PATH. Install via the latest release first."
  exit 0
fi
fluxmirror doctor
```

## Step 2: Output

Show the table verbatim to the user, then add a one-line summary in
`$USER_LANG` describing the overall state:

- `english`: "All checks passed." / "Some checks need attention."
- `korean`: "모든 점검 통과." / "일부 점검 항목 주의 필요."
- `japanese`: "すべてのチェックに合格。" / "一部のチェック項目に注意が必要です。"
- `chinese`: "所有检查均通过。" / "部分检查项需要关注。"

If `USER_LANG ≠ english`, also translate the column headers
(`component`, `status`, `detail`) below the raw table — keep the raw
table intact so the user can still grep / paste it.

## Step 3: Empty / missing data

If the binary is missing entirely, instruct the user (in chosen
language) to upgrade their plugin install: the `fluxmirror` binary is
auto-downloaded on first hook fire, so a fresh agent session usually
resolves it.
