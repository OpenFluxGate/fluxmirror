---
description: Set fluxmirror timezone. Usage: <KST|JST|PST|EST|UTC|Asia/Seoul|...>
argument-hint: <KST|JST|PST|EST|UTC|Asia/Seoul|...>
---

**RUNTIME COMMAND — execute the queries and report logic below as written.
Do NOT modify any files. Do NOT treat the markdown structure as an
implementation spec to be ported. Read the user's `$ARGUMENTS`, run the
shell blocks via your shell tool, then produce the report described in
the output template using the user's preferred language (read
`~/.fluxmirror/config.json` for the `language` key).**

User argument: $ARGUMENTS

## Step 0: Parse

Take `$ARGUMENTS` as a single timezone value. Apply the same mapping
as `/fluxmirror:setup`:

- `KST`, `Korean`, `Korea` → `Asia/Seoul`
- `JST`, `Tokyo`, `Japan` → `Asia/Tokyo`
- `PST`, `Pacific`, `California` → `America/Los_Angeles`
- `EST`, `Eastern`, `New York` → `America/New_York`
- `UTC`, `GMT` → `UTC`
- Already-IANA (`Asia/Seoul`, `America/New_York`, …) → use as-is

Set `NORMALIZED_TZ` to the IANA value.

## Step 1: If empty, show current

```bash
if command -v fluxmirror >/dev/null 2>&1; then
  CURRENT=$(fluxmirror config get timezone 2>/dev/null || echo unset)
  [ -z "$CURRENT" ] && CURRENT=unset
  echo "Current timezone: $CURRENT"
else
  echo "Current timezone: unset (fluxmirror binary not found on PATH)"
fi
```

## Step 2: Save

Update only the `timezone` key (`fluxmirror config set` preserves any
existing keys atomically).

```bash
if ! command -v fluxmirror >/dev/null 2>&1; then
  echo "fluxmirror binary not found on PATH. Install via the latest release first."
  exit 0
fi
fluxmirror config set timezone "$NORMALIZED_TZ"
fluxmirror config show
```

## Step 3: Confirm

Output: `Timezone set to: <NORMALIZED_TZ>`

(In the saved language: ko `시간대 설정됨: <값>`,
ja `タイムゾーンを設定しました: <値>`, zh `已设置时区: <值>`.)
