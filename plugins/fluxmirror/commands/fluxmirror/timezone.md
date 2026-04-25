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
CONFIG_FILE="$HOME/.fluxmirror/config.json"
if [ -f "$CONFIG_FILE" ]; then
  CURRENT=$(jq -r '.timezone // "unset"' "$CONFIG_FILE")
  echo "Current timezone: $CURRENT"
else
  echo "Current timezone: unset"
fi
```

## Step 2: Save

Preserve any existing `language` key. Only update `timezone`.

```bash
CONFIG_DIR="$HOME/.fluxmirror"
CONFIG_FILE="$CONFIG_DIR/config.json"
mkdir -p "$CONFIG_DIR"
[ -f "$CONFIG_FILE" ] || echo '{}' > "$CONFIG_FILE"

NEW=$(jq --arg tz "$NORMALIZED_TZ" '.timezone = $tz' "$CONFIG_FILE")
echo "$NEW" > "$CONFIG_FILE"
cat "$CONFIG_FILE"
```

## Step 3: Confirm

Output: `Timezone set to: <NORMALIZED_TZ>`

(In the saved language: ko `시간대 설정됨: <값>`,
ja `タイムゾーンを設定しました: <値>`, zh `已设置时区: <值>`.)
