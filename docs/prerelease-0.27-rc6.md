# 0.27.0-rc.6 preparation and acceptance boundary

## Version and authorization

The user corrected the target line to **0.27**, not 0.70, on 2026-11-12. This preparation uses **0.27.0-rc.6**, the next unused numbered RC observed at that time. Earlier 0.70/v070 worktree, audit and document filenames remain historical task identifiers, not application versions. No old tag, release or evidence is rewritten.

Read-only remote checks on 2026-11-12 found protected-main source a936b156d7f5d867ca082fe5c2fecf8e91b4a228 and the latest 0.27 publication [v0.27.0-rc.5](https://github.com/shawnwu2022/cc-desk/releases/tag/v0.27.0-rc.5), published at 2026-11-12T01:25:09Z as a prerelease. Listing all release pages found no rc.6 release/draft, and remote 0.27 tags stopped at rc.5. These observations do not reserve a version; formal admission must check them again.

This request authorizes local version, notes and validation preparation, **not** push, protected-main merge, workflow dispatch, tag or release creation. Changes remain on the project-local integration branch. The existing Release workflow can start on a main push that changes package.json; merging this prepared version is therefore part of a publication decision, not a harmless preparation step. No workflow, permissions, release policy or stable Latest setting is changed.

## Delivered candidate scope

Production integration was completed through d4e40149, with isolated OpenCode WebView acceptance added at 46f66e46 before this metadata/notes change.

- Windows Codex native archive uses scoped activity admission: the exact native target and related owners remain blocked; verified unrelated structured owners may coexist. Unknown/raw owners, conflicting mutation and maintenance still fail closed. This is not proof that independent external writers are idle.
- Windows Claude recovery moves only an identity-pinned main transcript into a same-volume persistent recovery area and permits no-overwrite restoration. Children, attachments and native indexes remain in place. It is not permanent deletion or a provider-native archive API.
- Windows OpenCode native archive/unarchive uses the bundled official metadata subset, bounded supported schema and exact source/session identity. Its separate authenticated panel requires confirmation. Native restore does not start a terminal or remove the separate Desk marker. Node.js 24.18 or newer and a supported local Windows/NTFS source are required; Unix mutation remains closed.
- Claude/OpenCode operations retain full Desk runtime quiescence. Codex's scoped concurrency exception does not extend to them. Unknown writes retain their ownership and pending identities, with read-only reconciliation and no replay.
- Canonical Windows rename paths and metadata-helper exit ownership are repaired. The independent scope-core CI harness now imports actual Claude recovery and actual OpenCode source-pinning logic rather than skipping their registry tests.

**Claude and OpenCode permanent deletion remains disabled and unfinished.** The reversible features do not close that requirement. Existing eligible Windows Codex permanent deletion keeps its separate preview, risk acknowledgments and one-shot confirmation. Do not describe this candidate as all TODOs completed.

## Evidence already obtained, with limits

- Production integration checks on d4e40149: 134 frontend tests, 26 Node policy tests, typecheck and production Vite build; 93 focused main-crate Rust passing executions including a real official-helper roundtrip on a fresh synthetic database; formatting and strict all-target Clippy. The independent scope-core suite passed 291 tests and strict Clippy. These suites overlap and are not one combined distinct-test total. They are local evidence, not full hosted CI.
- On 46f66e46, the isolated real Windows WebView host exercised production panels, authenticated IPC and the actual OpenCode Node helper against newly created synthetic records. The archive/restore run passed in 17.98 seconds. Desk archive/restore/rearchive and Claude recovery also ran in that host. Desk storage uses test-host capabilities rather than installed KnownFolder/ReplaceFileW IO.
- The OpenCode NoNative name means no native CLI or model launch, **not** no processes: the restricted Node metadata helper runs. Root target fields and non-target data were checked; event_sequence was not fully snapshotted, so this is not whole-database byte equality or complete external-writer acceptance.
- Integration rechecked 14 evidence hashes and the six changed source blobs, then passed 38 affected frontend tests, typecheck, fixture build with 98 production modules, script syntax, Rust format and diff checks. Independent read-only review found no blocking integration issue. The 120-second Native matrix was not repeated for that test-only commit.

Source-bound detailed records: [OpenCode WebView](../.audit/v070-opencode-webview.md), [its evidence pins](../.audit/opencode-webview-final-pins.json), [Desk persistence WebView](../.audit/v070-desk-persistence-webview.md) and [Claude recovery WebView](../.audit/v070-recycle-webview.md). Older records that called the OpenCode isolated WebView flow unfinished are superseded for that narrow flow only. They do not become installed-app or live-user acceptance.

## Remaining publication conditions

The unchanged [release process](release-process.md) is authoritative for every RC:

1. Obtain a concrete publication decision before remote submission/main promotion or any action that can trigger Release. Refresh current main, unused version, tag and all release/draft conflicts before promotion.
2. The actual merged protected-main SHA must pass its complete latest ordinary push CI and every required job, with authenticated coverage and original log bytes bound to that same SHA, run and attempt. Local focused tests, branch CI or old successful main runs cannot replace this evidence.
3. Exactly three platform artifacts must come from the same release workflow run and source and pass actual updater-signature verification. Keep the RC a GitHub prerelease with make_latest=false; verify the exact version manifest and all payload bytes/signatures after an authorized publication.
4. Keep the existing precise host disclosure policy: only the 18 reviewed Job-free specialists may be unverified when a successful host query proves the external Job condition. No new exclusions, softened assertions or claimed native All pass.

Complete installed-app acceptance, signed upgrade/return, real model lifecycles, complete descendants/external writers and D20/native All remain unverified. The release policy permits its precise disclosed host-unverified scope; it does not turn these product gaps into passes. The remaining product scope needs an explicit release decision and truthful disclosure, not an invented blanket waiver. Signing also does not establish Windows Authenticode or these behavioral acceptances.

## This preparation's minimal checks

The version change touches package/Cargo/Tauri root identities, matching literal expectations and bilingual documentation; it adds no runtime behavior. The checks below passed on the prepared version and notes. Full native matrices and signed builds are intentionally not repeated or triggered by this local preparation.


- Existing release recovery, native-release policy, authenticated coverage, draft-recovery and build-admission Node fixtures: **134 passed**, no skips (24.23 seconds).
- Version identities, generated bilingual notes/manifest and release URL Vitest tests: **16 passed** across three files.
- Vue/TypeScript typecheck passed. Direct production Vite build passed (1,031 modules, 5.04 seconds); prepared helper resources were reused, not reinstalled or freshly certified by this frontend build.
- Cargo 1.98.1 locked/offline metadata reports cc-desk 0.27.0-rc.6. All six version identities agree. Dependency package identities are unchanged.
- Generated notes match their JSON source, prior release highlights are unchanged, and the version parser classifies rc.6 as a prerelease. Diff whitespace checks passed.
- Existing npm user-config deprecation notices and Vite's large-chunk warning remain warnings, not test failures. No runtime source, workflow, release script or security setting changed.

The preparation logs are retained locally under .audit/v027-rc6-*.log. These results do not constitute a complete final-SHA hosted CI run, signed artifact set, install acceptance or publication approval. The prior Rust and WebView results remain bound to their recorded source; they are not relabeled as a freshly compiled rc.6 binary.
