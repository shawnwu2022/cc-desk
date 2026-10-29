# RPK checkpoint review dispositions

Independent read-only review: integration_transport_review; root owns final acceptance.
This records the supplied findings and actual corrections, not a security audit.

| Finding | Disposition / evidence under task/integration-evidence/security |
| --- | --- |
| Evidence validation occurred outside final deadline/cancel eligibility check | Fixed: finish validates then samples deadline/token before Pending. Actual RED04 -> GREEN05/14. Success linearizes at that sample, not physical return time. |
| watch<bool> cancellation could coalesce true then false | API replaced with private sticky flag and CancelHandle/Token; no reset/send exposed. Raw-watch finish/handshake RED07/08 retained; sticky API RED09 -> GREEN10/14. |
| Only first cancel sent wake: paused first caller could leave parked waiter asleep after second cancel returned | Every cancel unconditionally sets true and sends. Exact state-before-notify injection with parked waiter RED13 -> GREEN14. Not a concurrency stress claim. |
| Client handshake may finish before server rejects client credentials | Negative tests require the rejecting side to fail; Pending has no plaintext/permission API. AppBind confirmation remains OPEN. |
| Need both certificate-signature directions and both allowed ciphers | Both product signer corruption directions reject at target verifier; external test client independently forces AES and ChaCha against product accept. GREEN14. |
| Retain identity/deadline/cancel for next owner | Pending privately retains TLS, original deadline, sticky token and verified local/peer role-domain IDs. No public mutable/raw IO. |
| Default API off cannot be counted as RPK verification | Required-feature integration tests plus --all-features inventory16; default-off check17 is only disabled-build evidence. |
| “enrolled SPKI” implied registration was implemented | README corrected to expected SPKI; supplied pins/signers are backend inputs, enrollment is OPEN. |

Source frozen at the root's RPK review boundary; no AppBind fields, protocol minor/
security-floor mapping or TEST_ONLY domain was promoted from the lab. Public-known
private fixture material remains exclusively under tests. Root performs full workspace
all-features regression and any commit; this implementer has not committed this checkpoint.
