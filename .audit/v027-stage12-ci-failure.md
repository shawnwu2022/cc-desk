# RC preparation: original hosted frontend failure

PR: https://github.com/shawnwu2022/cc-desk/pull/81
Source head: `03132ba1162cb612a45a6ac73ddaa2514129e082`
Run: https://github.com/shawnwu2022/cc-desk/actions/runs/42457475491
Job: `128659468522` (Frontend checks), 2026-11-08 07:07:27Z-07:10:16Z.
Original job logs were read with the supported decoded workflow-job-log tool.

Observed failure: `tests/config/packageVersion.test.ts`.

```
PackageVersion_Identity_001
AssertionError: expected { npm: '0.27.0-rc.1', ... } to deeply equal { npm: '0.26.0', ... }

PackageVersion_Changelog_002
AssertionError: expected '0.27.0-rc.1' to be '0.26.0'

Test Files  1 failed | 210 passed (211)
Tests       2 failed | 3234 passed (3236)
Duration    135.97s
```

Typecheck passed; later Node policy and production CSP steps were skipped after
this real frontend failure. Those hosted skipped steps are not claimed passed.
This was caused by the authorized RC metadata change missing the pinned release
test update. The fix changes only the eight version/date literals; it does not
replace equality with a broad pattern or alter source/install/roundtrip guards.
The targeted `npx vitest run tests/config/packageVersion.test.ts` then passed all
5 tests; independent read-only review accepted the literal-only update. This
local result does not change the original hosted failure or prove new-source CI.

The first attempted CLI log read exited 1 because the overall run was still in
progress. A nonexistent release-notes test filename also exited 1 without running
tests (recorded in the RC preparation document). Neither is acceptance evidence.

An unrelated read of the branch-protection detail endpoint returned GitHub 403
`Resource not accessible by integration`; no retry, alternate identity or
protection change was attempted. The separately supported branch read had already
reported protected main. Publication must use the existing workflow's fresh gate.
