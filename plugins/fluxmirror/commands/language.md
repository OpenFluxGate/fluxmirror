---
description: Set fluxmirror output language. Usage: <korean|english|japanese|chinese>
argument-hint: <korean|english|japanese|chinese>
---

**RUNTIME COMMAND — execute the queries and report logic below as written.
Do NOT modify any files. Do NOT treat the markdown structure as an
implementation spec to be ported. Read the user's `$ARGUMENTS`, run the
shell blocks via your shell tool, then produce the report described in
the output template using the user's preferred language (read
`~/.fluxmirror/config.json` for the `language` key).**

User argument: $ARGUMENTS

## Step 0: Parse

Take `$ARGUMENTS` as a single language value. Apply the same mapping
as `/fluxmirror:setup`:

- `korean`, `ko`, `kr`, `한국어` → `korean`
- `english`, `en`, `영어` → `english`
- `japanese`, `ja`, `日本語` → `japanese`
- `chinese`, `zh`, `中文` → `chinese`

Set `NORMALIZED_LANG` to the canonical value.

## Step 1: If empty, show current

If `$ARGUMENTS` is empty, just print the current language and stop:

```bash
if command -v fluxmirror >/dev/null 2>&1; then
  CURRENT=$(fluxmirror config get language 2>/dev/null || echo unset)
  [ -z "$CURRENT" ] && CURRENT=unset
  echo "Current language: $CURRENT"
else
  echo "Current language: unset (fluxmirror binary not found on PATH)"
fi
```

## Step 2: Save

Update only the `language` key (`fluxmirror config set` preserves any
existing keys atomically).

```bash
if ! command -v fluxmirror >/dev/null 2>&1; then
  echo "fluxmirror binary not found on PATH. Install via the latest release first."
  exit 0
fi
fluxmirror config set language "$NORMALIZED_LANG"
fluxmirror config show
```

## Step 3: Confirm

Output: `Language set to: <NORMALIZED_LANG>`

(In the new language: ko `언어 설정됨: <값>`, ja `言語を設定しました: <値>`,
zh `已设置语言: <值>`.)
