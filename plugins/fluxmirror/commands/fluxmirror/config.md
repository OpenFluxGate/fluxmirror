---
description: Read / write / inspect FluxMirror config. Usage: [show|get <key>|set <key> <value>|explain]
argument-hint: [show|get <key>|set <key> <value>|explain]
---

**RUNTIME COMMAND — execute the queries and report logic below as written.
Do NOT modify any files. Do NOT treat the markdown structure as an
implementation spec to be ported. Read the user's `$ARGUMENTS`, run the
shell blocks via your shell tool, then produce the report described in
the output template using the user's preferred language (resolve via
`fluxmirror config get language`).**

User arguments: $ARGUMENTS

## Step 0: Load language preference

```bash
if command -v fluxmirror >/dev/null 2>&1; then
  USER_LANG=$(fluxmirror config get language 2>/dev/null || echo english)
else
  USER_LANG=english
fi
if [ -z "$USER_LANG" ]; then USER_LANG=english; fi
```

## Step 1: Parse + dispatch

Split `$ARGUMENTS` into tokens. The first token is the operation
(`show` | `get` | `set` | `explain`); default is `show` when no
arguments were supplied.

```bash
if ! command -v fluxmirror >/dev/null 2>&1; then
  echo "fluxmirror binary not found on PATH. Install via the latest release first."
  exit 0
fi

OP="${ARG1:-show}"   # the model substitutes the first token

case "$OP" in
  show)
    fluxmirror config show
    ;;
  get)
    # ARG2 = key
    fluxmirror config get "$ARG2"
    ;;
  set)
    # ARG2 = key, ARG3 = value
    fluxmirror config set "$ARG2" "$ARG3"
    echo ""
    echo "Updated config:"
    fluxmirror config show
    ;;
  explain)
    fluxmirror config explain
    ;;
  *)
    echo "Unknown op '$OP' (expected: show | get <key> | set <key> <value> | explain)"
    exit 0
    ;;
esac
```

## Step 2: Output

Pass the raw output through to the user. For `show` and `explain`,
optionally annotate any non-default values in the user's language. For
`set`, confirm in `$USER_LANG`:

- english: "Saved <key> = <value>."
- korean:  "저장됨: <key> = <value>."
- japanese: "保存しました: <key> = <value>."
- chinese:  "已保存: <key> = <value>."

## Step 3: Empty / missing data

If `get` returned nothing (exit 1) the requested key is unset; tell
the user that in their language and suggest the closest valid key
(language, timezone, wrapper.kind, storage.path, storage.retention_days).
