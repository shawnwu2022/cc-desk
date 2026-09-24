#!/bin/bash
# CC Desk Hook Reporter — side-channel only; every failure exits 0.
# Authenticated D13 runs use /observer. The capability is passed through a
# header file descriptor, never in curl argv or native/global config.

[ -z "$CC_BOX_HOOK_PORT" ] && exit 0
command -v curl >/dev/null 2>&1 || exit 0

if [ -n "$CC_DESK_OBSERVER_CAPABILITY" ] &&
   [ -n "$CC_DESK_OBSERVER_RUN" ] &&
   [ -n "$CC_DESK_OBSERVER_GENERATION" ]; then
  capability="$CC_DESK_OBSERVER_CAPABILITY"
  run_id="$CC_DESK_OBSERVER_RUN"
  generation="$CC_DESK_OBSERVER_GENERATION"
  event_id="$(date +%s 2>/dev/null)-$$-${RANDOM:-0}"

  unset CC_DESK_OBSERVER_CAPABILITY
  unset CC_DESK_OBSERVER_RUN
  unset CC_DESK_OBSERVER_GENERATION

  curl -s --max-time 3 -X POST "http://127.0.0.1:$CC_BOX_HOOK_PORT/observer" \
    -H "Content-Type: application/json" \
    -H @<(printf '%s\n' \
      "X-CC-Desk-Run: $run_id" \
      "X-CC-Desk-Generation: $generation" \
      "X-CC-Desk-Capability: $capability" \
      "X-CC-Desk-Event: $event_id" \
      "X-CC-Desk-Observer-Source: claude-hook") \
    --data-binary @- >/dev/null 2>&1
  exit 0
fi

# Legacy Claude-only compatibility path. It is not run authority.
curl -s --max-time 3 -X POST "http://127.0.0.1:$CC_BOX_HOOK_PORT/hook" \
  -H "Content-Type: application/json" \
  -H "X-CC-Box-Session: ${CC_BOX_SESSION_ID:-}" \
  -d @- >/dev/null 2>&1

exit 0
