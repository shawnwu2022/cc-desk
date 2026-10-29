# AppBind software-owner review record

Root owns final whole-workspace acceptance/commit. Independent read-only reviewer:
integration_transport_review. These are scoped engineering findings, not an audit.

| Concern | Disposition / actual evidence |
| --- | --- |
| Initial codec/owner absent | Codec actual3-test RED01 -> GREEN02; real TLS owner honest RED04 -> GREEN06. |
| URL parser erased forbidden empty-userinfo/percent-dot/control syntax; profile accepted empty capabilities | Actual2 review failures in03; raw2048/ASCII/@/% guard and exact capability singleton -> shared5/5 GREEN05. |
| Public wire DTO/Serde construction could bypass field constraints | Central field validators at decode/encode/selection/transcript/negotiate; bounds before transcript cloning; typed duplicate preservation and canonical byte equality. Invalid direct DTO/Unicode64 MAC checks pass. No runtime-panic RED is claimed. |
| Wire DTO mistaken for verified proof | BoundContext/BoundSelection have private fields, no Serde and readonly projections. Clones are historical metadata snapshots, never liveness/grants. |
| Owner might retain sender or fail to close peer | Owner holds only external sticky token, separate local oneshot. Last-handle drop/single-side cancel/owner Drop/close join and peer EOF tested with owned IO DropProbes (07). |
| Semantic fields signed by a legitimate key could be accepted blindly | Actual TLS attacks reject ID/keyepoch/origin/echo/profile/floor/caps/role/minor and genuinely MACed invalid selection; reflection/direction/order/malformed JSON reject (10/11). |
| Fresh application nonces could mask lack of exporter binding | Four fresh full TLS connections use exactly equal H/C/S/nonces/T: two normal successes with different MACs, then Host- and Client-confirm replay rejection. T matches independent Node digest (10/11). |
| Binding might renew timeout or ignore ciphertext backpressure | Actual RPK budget400ms consumed until80ms remain; no new deadline. True unread32-byte duplex blocks ciphertext flush, then expires; drip/half-close/cancel/abort/header bounds and owned cleanup tested (11). |
| Unsupported bytes silently accepted | Early control type rejects before owner; after valid four-message binding a held peer sends1byte, owner exits Unsupported without any dispatch API (11). |
| Only in-memory/model binding | Complete actual RPK+four AppBind messages through product bounded_tunnel/drive over loopback WS/mux,160 fragments, all owned tasks joined (12). Plain WS here, not composed outer WSS/admission. |

Clippy13 reported only cfg(test) type complexity/manual flatten; corrected with a
private test alias/iterator form, without suppressing checks. Root reruns strict checks.
Test nonce/mutation/capture fields are cfg(test)-private, never a production-enableable
test-support feature or public exporter/raw-TLS bypass. MAC/exporter/key material is
not logged. Root completes final full regression before any local checkpoint.
