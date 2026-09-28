# Native CLI Release Target Plan

`docs/testing/native-cli-release-targets.json` is the repository-owned authority for a release candidate's required native CLI acceptance coverage.

It is intentionally committed as `BLOCKED` until maintainers have defined the actual target combinations for a release.

## Schema

```json
{
  "schemaVersion": 1,
  "status": "READY",
  "targets": [
    {
      "targetId": "windows-11-x64-codex-<binary-hash-prefix>",
      "required": [
        {
          "caseId": "NATIVE-01",
          "evidenceLayers": ["C", "D"]
        },
        {
          "caseId": "NATIVE-63",
          "evidenceLayers": ["C"],
          "subcaseIds": ["observer-off", "observer-on"]
        }
      ]
    }
  ]
}
```

Rules:

- `status` must be `READY` for certification. `BLOCKED` prevents the gate from passing.
- `targetId` identifies one concrete target combination, not a product family.
- Every required case must list the evidence layers that are acceptable for that target.
- If a parent case has mandatory subcases, enumerate every `subcaseId`.
- A/B evidence must not be listed as acceptable for a requirement that needs real CLI or installed-package evidence.
- Cases that are not applicable are still required when they belong to the target plan; their record may use `N_A` only with explicit evidence and a reason.
- Changing this file after candidate creation changes a build/release input and therefore requires a new candidate.

## Relationship to acceptance evidence

`acceptance-manifest.json` repeats the target plan so the evidence bundle is self-describing, but D28 does not trust that copy.

The gate compares it with the target plan from the **exact candidate commit**. Any shrink, layer downgrade, missing subcase, or different target fails with `TARGET_PLAN_MISMATCH`.

## Current state

The checked-in file remains `BLOCKED` while real Claude Code / Codex CLI target certification is unavailable. This is deliberate: code-side CI can validate the gate implementation without enabling a real release.
