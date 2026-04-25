---
description: Configure fluxmirror language and timezone preferences
argument-hint: language <korean|english|japanese|chinese> timezone <KST|JST|PST|EST|UTC|Asia/Seoul|...>
---

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

If `$ARGUMENTS` contained neither keyword, just read and display the
existing config without modifying:

```bash
CONFIG_FILE="$HOME/.fluxmirror/config.json"
if [ -f "$CONFIG_FILE" ]; then
  echo "Current config:"
  cat "$CONFIG_FILE"
else
  echo "No config yet. Usage: /fluxmirror:setup language korean timezone KST"
fi
```

Then stop.

## Step 2: Merge and save

```bash
CONFIG_DIR="$HOME/.fluxmirror"
CONFIG_FILE="$CONFIG_DIR/config.json"
mkdir -p "$CONFIG_DIR"

if [ -f "$CONFIG_FILE" ]; then
  EXISTING=$(cat "$CONFIG_FILE")
else
  EXISTING='{}'
fi

NEW=$(echo "$EXISTING" | jq --arg lang "$NEW_LANG" --arg tz "$NEW_TZ" '
  (if $lang != "" then .language = $lang else . end)
  | (if $tz != "" then .timezone = $tz else . end)
')

echo "$NEW" > "$CONFIG_FILE"
echo "Saved to $CONFIG_FILE:"
cat "$CONFIG_FILE"
```

## Step 3: Confirm

Confirm to the user. Use the *new* (or pre-existing) language preference
for the confirmation message:

- English: `Saved: language=<lang>, timezone=<tz>`
- Korean: `저장됨: 언어=<lang>, 시간대=<tz>`
- Japanese: `保存しました: 言語=<lang>, タイムゾーン=<tz>`
- Chinese: `已保存: 语言=<lang>, 时区=<tz>`
