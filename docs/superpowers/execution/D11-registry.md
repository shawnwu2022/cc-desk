# D11 registry implementation checkpoint

## Observed RED

Source `6fa60e21b76cede56d27bf3763dd4586c463033e`, CI #164 run `35811770667`, PR merge `e7d8a71597b4de52b5a95f4a9aeb639c050938f4`, Rust job `107024705043`: compiled successfully; 439 passed / 17 failed / 15 ignored. All 17 new production-coordinator cases failed against the deliberate REGISTRY_NOT_IMPLEMENTED scaffold or its missing callback behavior. Full log was read before implementation. Clippy passed; formatting differences in the new scaffold/tests were a separate failure. Frontend job passed.

## Implementation rules

The registry admits caller identity before profile preparation, checks replay again at atomic reservation, and retains all accepted request IDs. Only the insertion winner receives a non-cloneable ticket. Route installation precedes the spawn linearization point; callbacks and resource destructors run outside the registry mutex. A route panic leaves Failed/Aborted, while a spawn panic leaves Indeterminate/OutcomeUnknown and does not reopen the tab for another process. Explicit callback errors are mapped to fixed stage enums without returning arbitrary diagnostic values.

Window epochs are backend-generated and monotone. The internal lifecycle API is NOT an IPC authentication mechanism by itself; live document-bound provenance remains required. Revocation before begin cancels; revocation after begin removes external authority while resources remain owned. Root exit is monotone and does not imply output drained. Retirement is an explicit backend action after launch completion and terminal state, retaining the replay tombstone.

Ruling: initial private request change detection uses two independently process-keyed standard-library hashes over canonical serde serialization. It is not a secret, token, signature or cryptographic authentication guarantee; authorization is separate. Tombstones do not retain the serialized prompt/environment. A subsequent bounds/integrity review remains required before live IPC exposure. No cross-process exactly-once guarantee is asserted.

D11 remains IN_PROGRESS: candidate implementation pending CI, frontend lost-response policy, live document-lifetime binding, actual process ownership integration and old-route migration. No native CLI, WebView or installer acceptance follows from generic-resource tests.
