# Quickstart — fluxmirror for Claude Code / Qwen Code

A 5-minute path from install to first daily report.

## 1. Install

Claude Code:

```
/plugin marketplace add OpenFluxGate/fluxmirror
/plugin install fluxmirror@fluxmirror
```

Qwen Code (the same plugin works for both):

```
qwen extensions install OpenFluxGate/fluxmirror:fluxmirror
```

## 2. Run init (interactive)

In a Claude Code or Qwen Code chat:

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

## Uninstall

Claude Code:

```
/plugin uninstall fluxmirror@fluxmirror
rm -rf ~/.fluxmirror
```

Qwen Code:

```
qwen extensions uninstall fluxmirror
rm -rf ~/.fluxmirror
```
