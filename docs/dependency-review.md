# Dependency maintenance evidence (2026-10-22)

Baseline: v0.24.0/main `24316ad9069df76f2de669fe7b17eeca2a2228be`.
The approved maintenance scope permits isolated dependency updates and local commits;
it does not authorize publication, paid model calls or new credentials.

## Advisory interpretation

The baseline npm lockfile audit reported 15 component records (two critical, nine
high, four moderate). These are upstream/package classifications, not 15 proven
CC Desk vulnerabilities. All initial static findings were `needs_review`; missing
reachability evidence is neither an exploit confirmation nor proof of safety.

| Component / baseline | Exposure prerequisite and inspected Desk boundary | Maintenance action |
| --- | --- | --- |
| `@intlify/core-base`, `vue-i18n` 9.14.4 | [HTML attribute interpolation advisory](https://github.com/intlify/vue-i18n/security/advisories/GHSA-x8qp-wqqm-57ph); inspected translation callers do not feed translated HTML to `v-html`. The observed `v-html` is fixed imported status SVG. | 9.14.5 together |
| `vue`, `@vue/server-renderer` 3.5.33 | [SSR attribute advisory](https://github.com/vuejs/core/security/advisories/GHSA-g2v6-rqmx-r4w6); no `renderToString` caller found in inspected `src`/scripts. A non-dev lock flag does not prove shipping reachability. | All Vue family packages resolve to 3.5.44 |
| `brace-expansion` 2.1.0 | Untrusted brace/glob input in development tooling; current 5.x copies are a separate dependency path. | Affected 2.x path resolves to 2.1.7 |
| `form-data` 4.0.5 | jsdom multipart field/filename input; no user-facing multipart route proven. | 4.0.6 |
| `nanoid` 3.3.11 | Attacker-controlled generator size; transitive PostCSS path is not itself a demonstrated runtime caller. | 3.3.20 |
| `postcss` 8.5.10 | Attacker-controlled CSS/sourceMappingURL during processing; compiler/build dependencies must be distinguished from rendered runtime. | 8.5.31 |
| `source-map-js` 1.2.1 | Attacker-controlled indexed source-map offsets; Vue/i18n compiler and PostCSS dependency paths. | 1.2.2 |
| `ws` 8.20.1 | Fragmented WebSocket input to an exposed server; inspected occurrence is jsdom tooling. | 8.22.0 |
| `esbuild` 0.21.5 | Development-server advisory; pinned session metadata helper calls `build`, not `serve`. | 0.25.12 in batch 2 |
| `vite` 5.4.21 | Dev-server/editor endpoint and platform-specific path prerequisites; default project server uses strict port 1420, without `host: true`. Alternate launches are not certified. | 6.4.4 in batch 2 |
| `vitest`, `@vitest/mocker` 3.2.4 | UI/API/mock-server exposure and controlled mock input. Normal project command is `vitest run`; that fact does not certify every optional invocation. | 4.1.12 in batch 3 |
| `tinypool` 1.1.1 | Upstream worker-option RCE gadgets additionally require prototype pollution/controlled options. Presence alone is not an exploit chain. | Removed by batch 3; no cross-major forced override |

## Batch 1: compatible Vue/i18n and transitive fixes

Manifest minimums become Vue `^3.5.42` and i18n `^9.14.5`; the fresh lock resolves
Vue 3.5.44 and all its internal exact peers together. Babel helper/parser updates
are transitive compiler dependencies. No SDK pin, test grouping, browser target,
application layout or runtime permission is changed. The initial resolver warning
referred to the old installed server-renderer peer; the isolated clean installation
and `npm ls` resolve a consistent new Vue family without an invalid peer tree.

`npm audit --package-lock-only --ignore-scripts --json` after this batch reports
five remaining records: `@vitest/mocker`, `esbuild`, `vite`, `vitest`, `tinypool`
(two moderate, one high, two critical). Audit counts can change with the database;
the original input and each local JSON result are retained separately, not rewritten.

The original checkout's `node_modules` remains unchanged. The implementation
checkout removed only its own junction nonrecursively and installed its own locked
dependencies with `npm ci --ignore-scripts`; it never installed through that junction.
Raw audit/test/build logs remain in the ignored local execution cache. Final bundle
reachability, toolchain migration and Rust advisory checks remain separate evidence.

## Batch 2: Vite 6 and pinned esbuild

Vite resolves to 6.4.4 and the direct metadata-helper compiler is pinned to esbuild
0.25.12. The existing Vue plugin 5.2.4 supports Vite 5/6. Explicit production targets
remain `es2021`, `chrome100`, `safari13`; SDK 0.3.301, its local-metadata-only bridge,
LICENSE and NOTICE are retained. See the version-specific
[Vite 5-to-6 migration guide](https://v6.vite.dev/guide/migration.html).

The first full run retained 13 failures: nine assertions assumed SVG imports were
file URLs, and four process tests exceeded five seconds during the initial parallel
Rust compilation. Test assertions now compare exact imported SVG resources rather
than filenames, retaining all theme/geometry/error/fallback checks and product code.
All 106 scoped component tests passed; the four process files passed 28 tests with
two original skips on recheck. The subsequent full run passed 3,128 tests across
198 files with the same two skips. No test timeout or grouping was changed.
Typechecking and pinned-helper preparation also passed.

Both English/Chinese production-startup browser tests passed under the actual CSP
using an isolated headless installed Chrome profile. The command's exit was zero,
but this restricted Windows run needed cleanup of its positively identified owned
preview server after assertions; automatic teardown is not certified by that run.
No user window or CLI was stopped. This is browser evidence, not installed WebView.
The remaining npm records are `@vitest/mocker`, `vitest`, `tinypool` (one moderate,
two critical package classifications), pending the separate Vitest migration.

## Batch 3: Vitest 4 without the old worker dependency

Vitest and its internal family resolve to 4.1.12, with no `tinypool` left in the
lockfile. Node 20/22/24 and Vite 6 remain compatible with the selected runner.
The [version-specific migration guide](https://v4.vitest.dev/guide/migration.html)
was checked; the four existing projects, file discovery, isolation, process tests,
5,000-session stress workload, assertions and original skips remain unchanged.

The initial full run had one failure: spying on jsdom's absent `matchMedia` method
is now rejected. That test explicitly stubs the missing browser API and restores
the global afterward, retaining reduced-motion and animation-cancellation assertions.
The separate visual fixture had one remaining SVG filename assumption; it now also
compares the exact imported resource, with no screenshot baseline update.

Verification: full frontend 198 files / 3,128 passed / two original skips; visual
unit 66 passed; original CI Node policy inventory 267 passed / six original skips;
typechecking passed. The fixed official SDK metadata test passed under normal task
permissions using only isolated synthetic JSONL and the owned helper; the restricted
token's earlier `NATIVE_RENAME_UNSUPPORTED` remains a separate failed host result.
No real user title or model turn was used.

Production Rollup chunk-module observation (positive rendered lengths) found i18n
core and message compiler in the shipped graph, while SSR/compiler-sfc, source-map-js,
PostCSS/nanoid and the reviewed development-server/test dependencies had no rendered
modules in that build. This is source/lock-bound build evidence, not proof about
every alternate build or the reachability of an exploit sink.

The final npm lockfile audit reports zero matching advisories at this observation.
This does not certify Rust, native binaries, arbitrary development commands, future
advisory databases or installed target behavior.

## Rust maintenance batch and remaining advisory inputs

The first full desktop-build attempt subsequently caught a missed cross-ecosystem
constraint: Rust Tauri 2.11.1 and the old npm API 2.10.1 fail the official CLI's
major/minor admission check. The npm API is now exactly pinned to 2.11.1, following
the [official dependency synchronization contract](https://v2.tauri.app/develop/updating-dependencies/).
`PackageVersion_TauriBuildAdmission_005` first failed on the mismatched lockfiles;
no `--ignore-version-mismatches` override is used. This correction updates one
isolated npm package and does not change product version or permissions.

The main package and all five independent test crates were queried against the
public OSV crates.io API using exact locked package versions, pagination, and
lockfile SHA-256 identities. This is an API observation, not a `cargo audit` run;
aliases and maintenance notices must not be counted as independent exploitable
Desk vulnerabilities. Final local evidence covers 727 distinct package versions
across six lockfiles. Its JSON and the initial result remain separately retained
in `.cache/next-version-execution/rust-advisories{,-final}.json`.

| Dependency | Locked change | Reason / verification boundary |
| --- | --- | --- |
| Tauri | 2.10.3 → 2.11.1 | [Upstream origin-confusion advisory](https://github.com/tauri-apps/tauri/security/advisories/GHSA-7gmj-67g7-phm9); exact navigation/document admission remains in place. No Desk-specific remote exploit was established. |
| rustls | 0.23.40 → 0.23.46 | Covers the matched handshake, debug/session, compression and downgrade advisory versions; updater TLS is a real client path, but individual feature/sink prerequisites differ. |
| rustls-webpki / webpki-root-certs | 0.103.13 → 0.103.16 / 1.0.7 → 1.0.10 | emSign distrust fixes. Linux system-root validation is relevant; the bundled-root edge was wasm-only in the observed graph, not the desktop root provider. |
| openssl | 0.10.78 → 0.10.80 | Compatible patch covering all matched advisories; Linux native TLS is present, but the named AES-KW/OCSP sinks were not found in Desk/native-tls usage. |
| anyhow / serde_with / tar | 1.0.102 → 1.0.103 / 3.18.0 → 3.21.0 / 0.4.45 → 0.4.46 | Compatible patches for downcast unsoundness, KeyValueMap panic and PAX handling. Independent test-crate pins/locks also updated. |

The Tauri runtime/build family is selected with Cargo's per-command
`resolver.incompatible-rust-versions="fallback"`, preserving declared Rust 1.89
compatibility rather than adopting runtime/build releases requiring 1.90. Actual
execution here used the installed newer compiler; an MSRV run is not claimed.
TLS backends, certificate checks, official-domain rules, updater minisign checks,
document proof, version-manager isolation and CLI process ownership are unchanged.
The generated Tauri ACL/schema changes are retained; three newly default-granted,
unused app/window queries are explicitly denied, as described in
[capabilities](capabilities.md). No additional tray permission is granted.

The old Tauri custom-protocol local-origin branch was affected upstream, but the
registered-user-protocol precondition was not found in this Desk configuration.
The built-in protocols are separate. Main and version-manager URL/document guards,
default new-window handling and current capabilities further constrain the path;
these are defense evidence, not a proof that every affected dependency is harmless.
The upgraded upstream branch checks the full `.localhost` suffix. No production
authorization was relaxed to keep an older dependency working.

Remaining matching inputs require the following dispositions rather than a forced
major-version override:

| Input | Source-backed disposition and next step |
| --- | --- |
| glib 0.18.5, [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html) | `needs_review` on Linux. The affected `VariantStrIter` exists. Its constructor is reached through `Variant::array_iter_str`; no production calls were found in Desk or the exact GIO/GTK/GDK/GdkPixbuf/WebKit sources inspected. This limited search does not prove all Linux paths unreachable. A compatible GTK/WebKit family migration and Linux target checks are needed before closure. |
| h2 0.3.27, [RUSTSEC-2026-0258](https://rustsec.org/advisories/RUSTSEC-2026-0258.html) | Present through reqwest 0.11/hyper 0.14. The observed native-TLS client lacks ALPN and manual HTTP/2 prior-knowledge usage; updater reqwest 0.13 also lacks its HTTP/2 feature. No affected network sink was established. Recheck when enabling those features; a major client migration cannot be replaced by forcing h2 0.4 underneath hyper 0.14. |
| quick-xml 0.38.4, [0194](https://rustsec.org/advisories/RUSTSEC-2026-0194.html) / [0195](https://rustsec.org/advisories/RUSTSEC-2026-0195.html) | plist uses ordinary Reader without the affected attribute/namespace APIs in the inspected path. No user-supplied XML sink was established. |
| quick-xml 0.39.2, same advisories | wayland-scanner 0.31.10 uses duplicate-checked attributes in a proc macro: the 0194 sink is reachable during builds when an attacker controls a protocol XML file. It is not runtime clipboard XML parsing. No NsReader/NamespaceResolver is used, so the 0195 API prerequisite is absent in this consumer. Track a compatible upstream scanner update; do not force quick-xml 0.40 into a 0.39 constraint. |
| rand 0.7.3, [RUSTSEC-2026-0097](https://rustsec.org/advisories/RUSTSEC-2026-0097.html) | phf_generator build-time SmallRng with fixed seed, not the advisory's reentrant ThreadRng/logger path. No exploit path found; retain upstream migration tracking. |
| zip 4.6.1, [0367](https://rustsec.org/advisories/RUSTSEC-2026-0367.html) / [0368](https://rustsec.org/advisories/RUSTSEC-2026-0368.html) / [0369](https://rustsec.org/advisories/RUSTSEC-2026-0369.html) | Windows updater optional ZIP extraction remains affected dependency code. Desk verifies official source/minisign and size/hash before the same bytes reach installation; arbitrary unsigned network ZIPs cannot reach this sink. Windows symlink behavior is relevant, so neither filename nor platform dismisses every issue. Upstream plugin constraint is zip 4; migrating to a fixed major requires compatible plugin work and installer regression. |
| fxhash, proc-macro-error, rustls-pemfile, serial, unic-* | Maintenance/deprecation inputs retained separately from exploitable security findings; track replacement through their owning dependencies. No unsafe override or automatic suppression added. |

Seven exact Linux source archives were fetched read-only from static.crates.io,
checked against Cargo.lock SHA-256, and only bounded source files read from tar
stdout. No crate was installed/executed. Their versions, URLs and hashes remain in
`.cache/next-version-execution/locked-linux-sources/provenance.json`; this does not
constitute a Linux runtime test. TLS and ZIP conclusions also depend on the current
feature graph and existing updater signature-authority trust boundary.

Verification after the Rust upgrade: 58 focused application tests passed
(document/manager admission, startup, updater HTTP/policy/redirect/source rejection,
official SDK metadata and OpenCode preparation); complete independent scope,
observer, input, transport and updater crates passed 327 tests with one original
observer ignore. Strict `cargo clippy --locked --all-targets -- -D warnings` and
formatting passed. Cargo reported a nonfatal existing global-cache cleanup warning
for a Windows `nul` filename; the cache/ACL was not modified. Installed WebView,
original nativeAll, Linux/macOS execution and a native origin-confusion reproducer
remain unverified, so complete target-level security verification is **blocked**,
not a certified vulnerability closure.
