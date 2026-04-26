---
description: First-run init. Probes wrapper engines and asks the user for language / timezone / wrapper interactively (any value already given on the command line is reused). Usage: [language <korean|english|japanese|chinese>] [timezone <KST|JST|PST|EST|UTC|Asia/Seoul|...>]
argument-hint: [language <korean|english|japanese|chinese>] [timezone <KST|JST|PST|EST|UTC|Asia/Seoul|...>]
---

**RUNTIME COMMAND — execute the queries and report logic below as written.
Do NOT modify any files. Do NOT treat the markdown structure as an
implementation spec to be ported. Read the user's `$ARGUMENTS`, run the
shell blocks via your shell tool, ask the user for any setting that was
not provided up front, then produce the report described in the output
template using the user's preferred language (read
`~/.fluxmirror/config.json` for the `language` key after init runs).**

User arguments: $ARGUMENTS

## Step 0: Parse arguments

`$ARGUMENTS` may contain `language <value>` and/or `timezone <value>`.
Both are optional — `init` works with neither. Anything not provided
here MUST be asked of the user in Step 2; do NOT silently pick a default.

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

Set `NEW_LANG` / `NEW_TZ` to the normalized values, or leave them empty
if the keyword was not given. Always leave `NEW_WRAPPER` empty at this
step — wrapper choice is collected in Step 2 only.

## Step 1: Probe wrapper engines

Find out which wrapper engines are actually available on this host so
the menu in Step 2 only offers possible choices.

```bash
if ! command -v fluxmirror >/dev/null 2>&1; then
  echo "fluxmirror binary not found on PATH. Install via the latest release first."
  exit 0
fi
fluxmirror wrapper probe
```

The output is a TSV (`engine`, `available`, `path`, …). Remember the
set of engines whose `available` column is `yes`. Build the wrapper
option list from those only.

## Step 2: Ask the user (INTERACTIVE — required)

This step is the whole point of init. NEVER skip it. NEVER fall back to
silent defaults. For each of the three settings, if it was not already
provided in `$ARGUMENTS`, ask the user.

Use the most natural interactive mechanism available in this runtime:
- **Claude Code** — call the `AskUserQuestion` tool with multi-choice
  options. Batch all needed questions into a single call when possible
  (the tool accepts up to 4 questions, up to 4 options each — "Other"
  is added automatically by the UI).
- **Gemini CLI / Qwen Code / any other host without `AskUserQuestion`** —
  ask in plain chat: print the question, list the options, wait for the
  user's reply, then continue. Do NOT proceed with assumed defaults.

Ask only what is still missing:

1. **Output language** — options: `korean`, `english`, `japanese`,
   `chinese`. Skip if `NEW_LANG` is already set from `$ARGUMENTS`.
2. **Timezone** — options: `Asia/Seoul (KST)`, `Asia/Tokyo (JST)`,
   `America/Los_Angeles (PST)`, `UTC`. The user may type any other IANA
   name via the "Other" path. Skip if `NEW_TZ` is already set.
3. **Wrapper engine** — options come from the Step 1 probe. Always
   include `auto-detect` as the last option so the user can defer to
   the binary's own recommendation. Recommend `bash` first if it is
   available, else `node`. Always ask this question — the wrapper is
   never settable via `$ARGUMENTS`.

Store the answers as `NEW_LANG`, `NEW_TZ`, `NEW_WRAPPER`. Map any IANA
free-text answers through the timezone table from Step 0. Map any
language free-text answers through the language table from Step 0.

## Step 3: Apply

Run init with the resolved language / timezone, then force the wrapper
engine if the user picked a specific one (skip `wrapper set` when the
user chose `auto-detect`):

```bash
args=(init --non-interactive)
if [ -n "$NEW_LANG" ]; then args+=(--language "$NEW_LANG"); fi
if [ -n "$NEW_TZ" ];   then args+=(--timezone "$NEW_TZ"); fi
fluxmirror "${args[@]}"

if [ -n "$NEW_WRAPPER" ] && [ "$NEW_WRAPPER" != "auto-detect" ] && [ "$NEW_WRAPPER" != "auto" ]; then
  fluxmirror wrapper set "$NEW_WRAPPER"
fi
```

## Step 4: Show the resulting state

```bash
echo
echo "Resolved config:"
fluxmirror config show
echo
echo "Health check:"
fluxmirror doctor
```

## Step 5: Confirm

Use the *resolved* `language` value (re-read after init) for the
confirmation message:

- English: `Initialized. language=<lang>, timezone=<tz>, wrapper=<kind>`
- Korean:  `초기화 완료. 언어=<lang>, 시간대=<tz>, 래퍼=<kind>`
- Japanese:`初期化しました。言語=<lang>, タイムゾーン=<tz>, ラッパー=<kind>`
- Chinese: `初始化完成。语言=<lang>, 时区=<tz>, 包装器=<kind>`

If `fluxmirror doctor` reported any `warn` or `fail` rows, list them
verbatim under the confirmation so the user can act on them.
