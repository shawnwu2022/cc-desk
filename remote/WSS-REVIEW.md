# WSS checkpoint: reviewed but not production acceptance

Remote workspace24/24 tests pass, strict Clippy/fmt pass. Independent read-only
reviewer inspected WSS/deadline/config/official fixtures and found no remaining
Critical/Important/Minor code defect in this increment. Two actual RED regressions
were corrected: invalid TLS name previously connected before rejecting; timeout_at
previously returned an already-ready success after expiry. TCP-before-name and
ready-work-after-deadline tests now reject correctly. No second review is claimed.

Initial3 WSS tests all RED at unimplemented code. Actual4 TLS tests demonstrate
TEST_ONLY valid chain/name/HTTP101 and CA/name/HTTP403 negatives. First silent-server
test hung because a deadline could precede DNS/connect while its server awaited accept;
system DNS work also is not canceled by dropping lookup future. Owned test executable
was stopped, its sockets/processes cleaned. Resolved-address API now avoids spawning
DNS; server fixture always aborts/joins its owned task. DNS remains OPEN, not fixed by
calling an OS blocking lookup under timeout. Production roots are never test-injected.

No persistent credential generation, system trust changes, real account or external
connection was used. Fixtures are universally known tokio-rustls0.26.4 source-package
test bytes, with licenses; not production identities. Normal client uses pinned
webpki-roots1.0.7. Root-bundle freshness/update and signed dependency/release policy
remain release gates; this pin alone is not a security audit or root freshness proof.

This checkpoint supplies usable outer WSS construction. It does not implement relay
admission, inner RPK/AppBind, Host authorization, mobile or deployment. IO deadline
rejects late results, but cannot preempt CPU or reverse bytes already sent.
