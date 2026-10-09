#!/bin/bash
# Optional observer: silent, bounded, no native hook decisions or retries.
# The CLI's five-second hook timeout is the outer deadline; curl has three seconds.
[ -n "$CC_BOX_HOOK_PORT" ] &&
[ -n "$CC_DESK_OBSERVER_CAPABILITY" ] &&
[ -n "$CC_DESK_OBSERVER_RUN" ] &&
[ -n "$CC_DESK_OBSERVER_GENERATION" ] || exit 0
case "$CC_BOX_HOOK_PORT" in *[!0-9]*|'') exit 0 ;; esac
command -v curl >/dev/null 2>&1 || exit 0
command -v od >/dev/null 2>&1 || exit 0
command -v tr >/dev/null 2>&1 || exit 0

# Assignment does not remove an inherited export attribute. Keep all private
# values out of child environments before invoking any external command.
export -n capability run_id generation event_id payload port
capability="$CC_DESK_OBSERVER_CAPABILITY"
run_id="$CC_DESK_OBSERVER_RUN"
generation="$CC_DESK_OBSERVER_GENERATION"
port="$CC_BOX_HOOK_PORT"
unset CC_DESK_OBSERVER_CAPABILITY CC_DESK_OBSERVER_RUN CC_DESK_OBSERVER_GENERATION

# C locale counts bytes. NUL or reaching byte 65,537 returns success: reject both.
# EOF returns nonzero and preserves whitespace, CR/LF, backslashes and UTF-8 bytes.
LC_ALL=C IFS= read -r -d '' -n 65537 payload && exit 0
[ -n "$payload" ] || exit 0
# Unique invocation identity is for replay suppression, not CLI event ordering.
event_id="$(od -An -N16 -tx1 /dev/urandom 2>/dev/null | tr -d ' \n')"
[ "${#event_id}" = 32 ] || exit 0

quote_config() {
  local value="$1"
  export -n value
  value=${value//\\/\\\\}
  value=${value//\"/\\\"}
  value=${value//$'\t'/\\t}
  value=${value//$'\n'/\\n}
  value=${value//$'\r'/\\r}
  value=${value//$'\v'/\\v}
  printf '"%s"' "$value"
}
# Windows-native curl cannot open Bash's /proc/.../fd process-substitution path.
# Stream escaped config through stdin instead: no files, secrets in argv, or
# double use of stdin. data-raw never interprets a leading @ as a file to read.
{
  for header in \
    'Content-Type: application/json' \
    "X-CC-Desk-Run: $run_id" \
    "X-CC-Desk-Generation: $generation" \
    "X-CC-Desk-Capability: $capability" \
    "X-CC-Desk-Event: $event_id" \
    'X-CC-Desk-Observer-Source: claude-hook'; do
    printf 'header = '; quote_config "$header"; printf '\n'
  done
  printf 'data-raw = '; quote_config "$payload"; printf '\n'
} | curl -q -s --max-time 3 --connect-timeout 1 --noproxy '*' --proto '=http' \
  -X POST "http://127.0.0.1:$port/observer" --config - >/dev/null 2>&1
exit 0
