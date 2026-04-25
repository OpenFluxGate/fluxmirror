---
description: Explain what fluxmirror is and list all available sub-commands
---

## Step 0: Load language preference

```bash
CONFIG_FILE="$HOME/.fluxmirror/config.json"

if [ -f "$CONFIG_FILE" ] && command -v jq >/dev/null 2>&1; then
  USER_LANG=$(jq -r '.language // empty' "$CONFIG_FILE")
fi

if [ -z "$USER_LANG" ]; then
  SYS=$(echo "${LANG:-en_US.UTF-8}" | cut -d_ -f1)
  case "$SYS" in
    ko) USER_LANG="korean" ;;
    ja) USER_LANG="japanese" ;;
    zh) USER_LANG="chinese" ;;
    *)  USER_LANG="english" ;;
  esac
fi

echo "Language: $USER_LANG"
```

## Step 1: Discover sub-commands dynamically

To prevent the listing from going stale, enumerate the actual command
files instead of hardcoding names. Commands live under
`commands/fluxmirror/` (sub-namespace so that Qwen / Gemini also
register them as `/fluxmirror:<name>`):

```bash
PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-$(dirname "$0")/../..}"
CMD_DIR="$PLUGIN_ROOT/commands/fluxmirror"

if [ -d "$CMD_DIR" ]; then
  echo ""
  echo "=== Available sub-commands ==="
  for f in "$CMD_DIR"/*.md; do
    name=$(basename "$f" .md)
    desc=$(awk '/^description:/ {sub(/^description: */,""); print; exit}' "$f")
    printf "  /fluxmirror:%-14s %s\n" "$name" "$desc"
  done
fi
```

## Step 2: Explain in natural prose

In `$USER_LANG`, write a single flowing explanation under 200 words.
No bullet-point dump in the prose itself — natural sentences. End with
the dynamically-listed sub-commands from Step 1 verbatim.

Cover briefly:

- What fluxmirror is: a multi-agent activity audit for Claude Code,
  Qwen Code, Gemini CLI, and (optionally) Claude Desktop MCP traffic.
- How it works: PostToolUse / AfterTool hooks write each tool call to
  the `agent_events` SQLite table; the optional Rust `fluxmirror-proxy`
  binary captures Claude Desktop's stdio JSON-RPC traffic into the
  `events` table.
- Where data lives:
  `$HOME/Library/Application Support/fluxmirror/events.db`.
- That the sub-commands listed below are the full surface (no other
  hidden commands).

If `USER_LANG=korean`, write in Korean. If `english`, write in English.
For `japanese` and `chinese`, translate naturally.
