#!/bin/bash
# CC Desk observer only: silent, bounded, never changes native hook decisions.
# The CLI's hook timeout (5 seconds) is the outer deadline. curl has a 3-second
# deadline; at most 64 KiB + 1 stdin bytes are read. The extra byte makes an
# oversized event fail as a whole, rather than accepting truncated JSON.
[ -n "$CC_BOX_HOOK_PORT" ] &&
[ -n "$CC_DESK_OBSERVER_CAPABILITY" ] &&
[ -n "$CC_DESK_OBSERVER_RUN" ] &&
[ -n "$CC_DESK_OBSERVER_GENERATION" ] || exit 0
case "$CC_BOX_HOOK_PORT" in *[!0-9]*|'') exit 0 ;; esac
command -v curl >/dev/null 2>&1 || exit 0
command -v head >/dev/null 2>&1 || exit 0
command -v od >/dev/null 2>&1 || exit 0
command -v tr >/dev/null 2>&1 || exit 0

capability="$CC_DESK_OBSERVER_CAPABILITY"
run_id="$CC_DESK_OBSERVER_RUN"
generation="$CC_DESK_OBSERVER_GENERATION"
# Independent hook invocations have no authoritative source ordering.
# This identifier is for duplicate delivery suppression, not a sequence number.
event_id="$(od -An -N16 -tx1 /dev/urandom 2>/dev/null | tr -d ' \n')"
[ "${#event_id}" = 32 ] || exit 0
# An inherited variable can already have the export attribute. Assignment alone
# does not make it private; explicitly remove that attribute before exec.
export -n capability run_id generation event_id
unset CC_DESK_OBSERVER_CAPABILITY CC_DESK_OBSERVER_RUN CC_DESK_OBSERVER_GENERATION

# -q is first: user curl configuration must not redirect this local capability.
# The secret is in a pipe, never an argv slot. Do not retry failed observations.
head -c 65537 2>/dev/null | curl -q -s --max-time 3 --connect-timeout 1 \
  --noproxy '*' --proto '=http' -X POST "http://127.0.0.1:$CC_BOX_HOOK_PORT/observer" \
  -H 'Content-Type: application/json' \
  -H @<(printf '%s\n' \
    "X-CC-Desk-Run: $run_id" \
    "X-CC-Desk-Generation: $generation" \
    "X-CC-Desk-Capability: $capability" \
    "X-CC-Desk-Event: $event_id" \
    'X-CC-Desk-Observer-Source: claude-hook') \
  --data-binary @- >/dev/null 2>&1
exit 0
