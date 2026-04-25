---
description: Set fluxmirror output language
argument-hint: <korean|english|japanese|chinese>
---

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
CONFIG_FILE="$HOME/.fluxmirror/config.json"
if [ -f "$CONFIG_FILE" ]; then
  CURRENT=$(jq -r '.language // "unset"' "$CONFIG_FILE")
  echo "Current language: $CURRENT"
else
  echo "Current language: unset"
fi
```

## Step 2: Save

Preserve any existing `timezone` key. Only update `language`.

```bash
CONFIG_DIR="$HOME/.fluxmirror"
CONFIG_FILE="$CONFIG_DIR/config.json"
mkdir -p "$CONFIG_DIR"
[ -f "$CONFIG_FILE" ] || echo '{}' > "$CONFIG_FILE"

NEW=$(jq --arg lang "$NORMALIZED_LANG" '.language = $lang' "$CONFIG_FILE")
echo "$NEW" > "$CONFIG_FILE"
cat "$CONFIG_FILE"
```

## Step 3: Confirm

Output: `Language set to: <NORMALIZED_LANG>`

(In the new language: ko `언어 설정됨: <값>`, ja `言語を設定しました: <値>`,
zh `已设置语言: <值>`.)
