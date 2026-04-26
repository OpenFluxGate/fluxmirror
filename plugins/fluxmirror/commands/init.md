---
description: First-run init. Picks a wrapper engine and writes language/timezone defaults. Usage: [language <korean|english|japanese|chinese>] [timezone <KST|JST|PST|EST|UTC|Asia/Seoul|...>]
argument-hint: [language <korean|english|japanese|chinese>] [timezone <KST|JST|PST|EST|UTC|Asia/Seoul|...>]
---

**RUNTIME COMMAND — execute the queries and report logic below as written.
Do NOT modify any files. Do NOT treat the markdown structure as an
implementation spec to be ported. Read the user's `$ARGUMENTS`, run the
shell blocks via your shell tool, then produce the report described in
the output template using the user's preferred language (read
`~/.fluxmirror/config.json` for the `language` key after init runs).**

User arguments: $ARGUMENTS

## Step 0: Parse arguments

`$ARGUMENTS` may contain `language <value>` and/or `timezone <value>`. Both
are optional — `init` works with neither.

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
- Already-IANA values (e.g., `Asia/Seoul`) → use as-is

Set `NEW_LANG` / `NEW_TZ` to the normalized values, or empty string if
the keyword was not given.

## Step 1: Run init non-interactively

The slash-command context cannot answer interactive prompts, so always
pass `--non-interactive`. The binary's wizard then probes the host for
available wrapper engines, picks the recommended one, and writes config
defaults — preserving any value the user already set unless overridden.

```bash
if ! command -v fluxmirror >/dev/null 2>&1; then
  echo "fluxmirror binary not found on PATH. Install via the latest release first."
  exit 0
fi

args=(init --non-interactive)
if [ -n "$NEW_LANG" ]; then args+=(--language "$NEW_LANG"); fi
if [ -n "$NEW_TZ" ];   then args+=(--timezone "$NEW_TZ"); fi

fluxmirror "${args[@]}"
```

## Step 2: Show the resulting state

```bash
echo
echo "Resolved config:"
fluxmirror config show
echo
echo "Health check:"
fluxmirror doctor
```

## Step 3: Confirm

Use the *resolved* `language` value (re-read after init) for the
confirmation message:

- English: `Initialized. language=<lang>, timezone=<tz>, wrapper=<kind>`
- Korean:  `초기화 완료. 언어=<lang>, 시간대=<tz>, 래퍼=<kind>`
- Japanese:`初期化しました。言語=<lang>, タイムゾーン=<tz>, ラッパー=<kind>`
- Chinese: `初始化完成。语言=<lang>, 时区=<tz>, 包装器=<kind>`

If `fluxmirror doctor` reported any `warn` or `fail` rows, list them
verbatim under the confirmation so the user can act on them.
