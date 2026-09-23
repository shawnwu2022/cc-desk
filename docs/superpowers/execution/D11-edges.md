# D11 bounds and response ordering tests

Seven additional coordinator tests are introduced before implementing new bounds or receipt fields. Three specify new behavior and must be observed RED: routing IDs no more than 128 UTF-8 bytes/no controls before preparation; canonical request fingerprint serialization limited to 8 MiB with explicit REQUEST_TOO_LARGE and no truncation; backend instance plus canonical u64 receipt revision for stale-response ordering. Four strengthen existing expected behavior: identical-body wrong snapshot owner, reentrant destructor outside global lock, preparation failure after another caller wins, and resource retirement before spawn completion.

Ruling: the 8 MiB limit is a managed launch-request admission budget, not a prompt/paste limit or normalization. Raw argv is never shortened. CLI/TUI input has its own later transport contract. Hash request serialization incrementally rather than copying unbounded raw prompts into a second Vec. All routing metadata retained by the bounded record count also needs bounded length.

The frontend must not use phase names alone to order replies; a later query can finish before an earlier start response. Backend instance IDs distinguish a restarted registry and revisions distinguish updates within a run. These additions do not establish live document authorization or cross-backend exactly-once execution.
