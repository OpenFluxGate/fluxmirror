# Quickstart — fluxmirror for Gemini CLI

A 5-minute path from install to first daily report.

## 1. Install

```
gemini extensions install https://github.com/OpenFluxGate/fluxmirror \
  --ref gemini-extension-pkg --consent
```

## 2. Run init (interactive)

In Gemini chat:

```
/fluxmirror:init
```

Answer three questions on one line, e.g. `english Asia/Seoul bash`.

## 3. Trigger any tool call (read a file, run a command), then:

```
/fluxmirror:today
```

You'll see a per-agent activity table immediately — the demo row
inserted by init guarantees the report is non-empty even before
your first real tool call lands.

## Cheat sheet

```
/fluxmirror:today        today
/fluxmirror:yesterday    yesterday
/fluxmirror:week         last 7 days
/fluxmirror:agents       per-agent 7-day totals
/fluxmirror:compare      today vs yesterday
/fluxmirror:about        what is this + sub-command list
/fluxmirror:doctor       health check
```

## Where data lives

`~/Library/Application Support/fluxmirror/events.db` (mac) /
`~/.local/share/fluxmirror/events.db` (Linux) /
`%APPDATA%\fluxmirror\events.db` (Windows).

## Weekly digest card

Generate a single self-contained HTML page summarising the last 7 days
of activity:

```bash
fluxmirror week --format html --out ~/Desktop/week.html
```

Open `~/Desktop/week.html` in a browser. The file embeds all CSS
inline — no network or server needed. Drop it into a Slack DM, attach
it to a Notion page, or just keep it as a private journal.

## Uninstall

```
gemini extensions uninstall fluxmirror
rm -rf ~/.fluxmirror
```
