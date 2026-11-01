# Durable input cursor candidate

`InputCursor {profile:1, request:"N"}` is a factual query through the existing
installed native Client channel. `N` shares the connection's monotonic request
counter. It creates no view, snapshot ACK, input window, grant or effect.

The explicit reply is `InputCursorState` with the same `profile` and `request`,
plus required nullable `nextSequence` and `pending` fields. Exactly one is nonnull:

- `nextSequence` is a canonical nonzero u64 decimal string. The Host derives it
  from the durable admitted floor plus one only after verifying no unsettled row.
- `pending` contains the exact durable `operation`, `sequence`, `outcome`, `proof`.
  HostWritten/Rejected require their retained receipt proof; Unknown/Partial have
  null proof and remain blocked. Final receipts must be explicitly settled through
  the existing exact receipt ACK path before another sequence is offered.

The Host derives target and device from its original authorized peer. A caller
cannot select a journal scope. The new action runs in the original writer actor
order with prepare and settlement, reusing `single_unsettled` and receipt checks.
No database schema changes or database transaction across IO are introduced.
Admitted greater than settled with a missing/multiple/invalid row, overflow,
poisoned storage or a schema mismatch fails closed. A cursor read never repairs or
discards journal facts and never retries an input effect.

Original channel, scope, fence, actor scope identity, grant expiry and revocation
are checked before delivery. Cursor retains the original bounded receiver custody
on cancellation just like receipt queries. It is factual metadata, so it does not
restart the input lease; actual input still requires the existing live lease,
snapshot ACK, current window and no different unresolved operation.

NativeClient accepts only a well-formed InputCursorState for the exact request.
WindowGranted or other old replies cannot stand in for a cursor. All existing
profile-1 message fields remain unchanged. An older peer rejects the new unknown
request tag, closes the channel and grants no input permission.

Tests use temporary SQLite files, memory writers, public TEST_ONLY identities and
in-memory TLS. The optional existing `CC_DESK_TEST_ONLY_CONTROL_JSONL` test sink
uses create_new to export two actual Rust encoder replies for client compatibility
checks. This cut supplies no production key provider, remote enrollment or CLI
compatibility qualification.
