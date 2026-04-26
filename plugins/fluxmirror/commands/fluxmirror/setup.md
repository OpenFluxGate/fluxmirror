---
description: Configure fluxmirror. Usage: language <korean|english|japanese|chinese> timezone <KST|JST|PST|EST|UTC|Asia/Seoul|...>
argument-hint: language <korean|english|japanese|chinese> timezone <KST|JST|PST|EST|UTC|Asia/Seoul|...>
---

**RUNTIME COMMAND — execute the queries and report logic below as written.
Do NOT modify any files. Do NOT treat the markdown structure as an
implementation spec to be ported. Read the user's `$ARGUMENTS`, run the
shell blocks via your shell tool, then produce the report described in
the output template using the user's preferred language (read
`~/.fluxmirror/config.json` for the `language` key).**

User arguments: $ARGUMENTS

## Step 0: Parse arguments

Look in `$ARGUMENTS` for the keywords `language <value>` and/or
`timezone <value>`. Either, both, or neither may be present.

**Language mapping** (normalize to lowercase canonical):
- `korean`, `ko`, `kr`, `한국어` → `korean`
- `english`, `en`, `영어` → `english`
- `japanese`, `ja`, `日本語` → `japanese`
- `chinese`, `zh`, `中文` → `chinese`

**Timezone mapping** (normalize to IANA):
- `KST`, `Korean`, `Korea` → `Asia/Seoul`
- `JST`, `Tokyo`, `Japan` → `Asia/Tokyo`
- `PST`, `Pacific`, `California` → `America/Los_Angeles`
- `EST`, `Eastern`, `New York` → `America/New_York`
- `UTC`, `GMT` → `UTC`
- Already-IANA values (e.g., `Asia/Seoul`, `America/New_York`) → use as-is

Set `NEW_LANG` and `NEW_TZ` to the normalized values, or empty string if
the corresponding keyword was not given.

## Step 1: If both empty, show current and stop

If `$ARGUMENTS` contained neither keyword, just print the resolved
config and stop:

```bash
if command -v fluxmirror >/dev/null 2>&1; then
  echo "Current config:"
  fluxmirror config show
else
  echo "fluxmirror binary not found on PATH. Install via the latest release first."
fi
```

Then stop.

## Step 2: Merge and save

`fluxmirror config set` preserves all unrelated keys atomically — call
it once per provided value (skip the call if the corresponding string
is empty):

```bash
if ! command -v fluxmirror >/dev/null 2>&1; then
  echo "fluxmirror binary not found on PATH. Install via the latest release first."
  exit 0
fi
if [ -n "$NEW_LANG" ]; then
  fluxmirror config set language "$NEW_LANG"
fi
if [ -n "$NEW_TZ" ]; then
  fluxmirror config set timezone "$NEW_TZ"
fi
echo "Saved:"
fluxmirror config show
```

## Step 3: Confirm

Confirm to the user. Use the *new* (or pre-existing) language preference
for the confirmation message:

- English: `Saved: language=<lang>, timezone=<tz>`
- Korean: `저장됨: 언어=<lang>, 시간대=<tz>`
- Japanese: `保存しました: 言語=<lang>, タイムゾーン=<tz>`
- Chinese: `已保存: 语言=<lang>, 时区=<tz>`
