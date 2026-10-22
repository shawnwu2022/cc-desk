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
| `esbuild` 0.21.5 | Development-server advisory; pinned session metadata helper calls `build`, not `serve`. | Separate toolchain batch pending |
| `vite` 5.4.21 | Dev-server/editor endpoint and platform-specific path prerequisites; default project server uses strict port 1420, without `host: true`. Alternate launches are not certified. | Separate toolchain batch pending |
| `vitest`, `@vitest/mocker` 3.2.4 | UI/API/mock-server exposure and controlled mock input. Normal project command is `vitest run`; that fact does not certify every optional invocation. | Separate runner migration pending |
| `tinypool` 1.1.1 | Upstream worker-option RCE gadgets additionally require prototype pollution/controlled options. Presence alone is not an exploit chain. | Remove through compatible runner migration; no cross-major forced override |

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
