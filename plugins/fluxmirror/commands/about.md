---
description: Explain what fluxmirror is and list all available sub-commands
---

**RUNTIME COMMAND — execute the queries and report logic below as written.
Do NOT modify any files. Do NOT treat the markdown structure as an
implementation spec to be ported. Read the user's `$ARGUMENTS`, run the
shell blocks via your shell tool, then produce the report described in
the output template using the user's preferred language (read
`~/.fluxmirror/config.json` for the `language` key).**

## Step 0: Load language preference

```bash
if command -v fluxmirror >/dev/null 2>&1; then
  USER_LANG=$(fluxmirror config get language 2>/dev/null || echo english)
else
  USER_LANG=english
fi
if [ -z "$USER_LANG" ]; then USER_LANG=english; fi

echo "Language: $USER_LANG"
```

## Step 1: Discover sub-commands dynamically

To prevent the listing from going stale, enumerate the actual command
files instead of hardcoding names. Commands live directly under
`commands/`; Claude Code / Qwen Code register each `<name>.md` as
`/fluxmirror:<name>`:

```bash
PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-$(dirname "$0")/..}"
CMD_DIR="$PLUGIN_ROOT/commands"

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
