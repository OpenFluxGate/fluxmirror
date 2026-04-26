#!/usr/bin/env bash
# Regenerate hooks.json files from manifests/source.yaml.
#
# Usage:
#   scripts/build-manifests.sh           regenerate all output files
#   scripts/build-manifests.sh --check   diff against committed; exit 1 on drift

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SOURCE="$REPO_ROOT/manifests/source.yaml"

CHECK_MODE=0
if [ "${1:-}" = "--check" ]; then
    CHECK_MODE=1
fi

if [ ! -f "$SOURCE" ]; then
    echo "error: manifests/source.yaml not found at $SOURCE" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Hand-parse source.yaml.  The file is small and regular:
#   agents:
#     <name>:
#       hook_event: <VALUE>
#       plugin_var: <VALUE>
#       output: <VALUE>
#       kind: <VALUE>
# We collect one agent block at a time and emit its hooks.json.
# ---------------------------------------------------------------------------

# generate_hooks_json <hook_event> <plugin_var> <kind>
generate_hooks_json() {
    local hook_event="$1"
    local plugin_var="$2"
    local kind="$3"
    printf '{\n'
    printf '    "hooks": {\n'
    printf '        "%s": [\n' "$hook_event"
    printf '            {\n'
    printf '                "hooks": [\n'
    printf '                    {\n'
    printf '                        "type": "command",\n'
    printf '                        "command": "${%s}/wrappers/router.sh %s"\n' "$plugin_var" "$kind"
    printf '                    }\n'
    printf '                ]\n'
    printf '            }\n'
    printf '        ]\n'
    printf '    }\n'
    printf '}\n'
}

# Parse the YAML and process each agent block.
# State machine: track current indent context via leading-space count.
current_agent=""
hook_event=""
plugin_var=""
output=""
kind=""

# We emit an agent's output whenever we hit a new top-level agent name or EOF.
flush_agent() {
    if [ -z "$current_agent" ]; then
        return
    fi
    if [ -z "$hook_event" ] || [ -z "$plugin_var" ] || [ -z "$output" ] || [ -z "$kind" ]; then
        echo "error: incomplete entry for agent '$current_agent' in source.yaml" >&2
        exit 1
    fi

    local dest="$REPO_ROOT/$output"
    local content
    content="$(generate_hooks_json "$hook_event" "$plugin_var" "$kind")"

    if [ "$CHECK_MODE" -eq 1 ]; then
        local tmpfile
        tmpfile="$(mktemp)"
        printf '%s\n' "$content" > "$tmpfile"
        if ! diff -u "$dest" "$tmpfile" > /dev/null 2>&1; then
            echo "DRIFT: $output" >&2
            diff -u "$dest" "$tmpfile" >&2 || true
            rm -f "$tmpfile"
            return 1
        fi
        rm -f "$tmpfile"
    else
        local dir
        dir="$(dirname "$dest")"
        mkdir -p "$dir"
        local tmpfile
        tmpfile="$(mktemp "$dir/.hooks.json.XXXXXX")"
        printf '%s\n' "$content" > "$tmpfile"
        mv "$tmpfile" "$dest"
        echo "wrote $output"
    fi
}

# Track whether any check failed.
check_failed=0

# Read source.yaml line by line.
# We need to detect agent name lines (4-space indent + identifier + colon)
# and key/value lines (8-space indent + key: value).
in_agents_block=0

while IFS= read -r line || [ -n "$line" ]; do
    # Strip trailing whitespace (including \r for safety).
    line="${line%$'\r'}"
    line="${line%  }"
    line="${line% }"

    # Skip blank lines and comment lines.
    case "$line" in
        ''|\#*) continue ;;
    esac

    # Detect the top-level "agents:" key.
    if [ "$line" = "agents:" ]; then
        in_agents_block=1
        continue
    fi

    if [ "$in_agents_block" -eq 0 ]; then
        continue
    fi

    # Count leading spaces to determine depth.
    stripped="${line#"${line%%[! ]*}"}"
    spaces=$(( ${#line} - ${#stripped} ))

    # Depth 2 (4 spaces): agent name line  e.g. "  claude-code:"
    # Depth 3 (6 spaces): but our YAML uses 4-space indent per level, so
    #   agent names are at 2 spaces, fields at 4 spaces.
    # Actual indents in source.yaml:
    #   "  claude-code:"  -> 2 spaces
    #   "    hook_event:" -> 4 spaces
    if [ "$spaces" -eq 2 ]; then
        # Flush previous agent (may set check_failed).
        flush_agent || check_failed=1
        # Start new agent.  Strip leading spaces and trailing colon.
        current_agent="${stripped%:}"
        hook_event=""
        plugin_var=""
        output=""
        kind=""
        continue
    fi

    if [ "$spaces" -eq 4 ]; then
        key="${stripped%%:*}"
        value="${stripped#*: }"
        case "$key" in
            hook_event) hook_event="$value" ;;
            plugin_var) plugin_var="$value" ;;
            output)     output="$value" ;;
            kind)       kind="$value" ;;
        esac
        continue
    fi
done < "$SOURCE"

# Flush the last agent.
flush_agent || check_failed=1

if [ "$CHECK_MODE" -eq 1 ]; then
    if [ "$check_failed" -eq 1 ]; then
        echo "build-manifests --check: drift detected (see diff above)" >&2
        exit 1
    else
        echo "build-manifests --check: all files match source"
        exit 0
    fi
fi
