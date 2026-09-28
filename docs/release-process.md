# CC Desk Release Process

The stable release path is fail-closed. Source changes, tags, or version-file changes do **not** publish a release by themselves.

## 1. Freeze source

Before building a candidate:

- finish all code and documentation changes;
- keep `package.json`, `src-tauri/Cargo.toml`, `Cargo.lock`, `package-lock.json`, and `src-tauri/tauri.conf.json` consistent when a release version is intentionally changed;
- record the exact 40-hex commit SHA;
- do not change build inputs after candidate creation. Any build-input change requires a new candidate.

## 2. Build signed candidate packages

Run **Signed candidate packages** manually.

The workflow:

1. builds Windows x64, Linux x64, and macOS ARM packages;
2. requires the updater signing secrets;
3. uploads signed platform artifacts;
4. downloads those artifacts into the manifest job;
5. runs `scripts/native-cli/candidate-manifest.mjs`;
6. uploads one `candidate-manifest.json` containing:
   - `candidateId`;
   - source commit SHA;
   - relative artifact path;
   - artifact type;
   - SHA-256;
   - exact byte size.

This workflow has read-only repository permissions and has no release-publishing step.

## 3. Collect acceptance evidence

Install the **same candidate files** identified by the candidate manifest on the declared target combinations.

Acceptance evidence must use the D28 schema consumed by:

```bash
node scripts/native-cli/verify-acceptance.mjs acceptance-manifest.json candidate-manifest.json docs/testing/native-cli-release-targets.json <evidence-root>
```

The gate rejects, among other failures:

- a release target plan that is not explicitly `READY`;
- evidence targets/layers that differ from the target plan frozen in the candidate commit;
- missing target/case/subcase evidence;
- duplicate records;
- undeclared records;
- `FAIL`, `BLOCKED`, or `NOT_RUN`;
- `N_A` without explicit evidence and reason;
- candidate identity mismatch;
- missing, symlinked, oversized, or hash-mismatched evidence files;
- D-layer package hashes not present in the candidate manifest.

A/B layer tests cannot substitute for required C/D evidence.

## 4. Run the acceptance gate

Use **Native CLI acceptance gate** with the explicit candidate workflow run ID plus the evidence workflow run ID/artifact that contain `acceptance-manifest.json`.

The gate first downloads the candidate manifest from the explicit candidate run, checks out that exact candidate commit, then loads the repository-owned target plan from that commit. It downloads evidence from the explicit evidence run and re-hashes every referenced evidence file. It does not infer “latest”, search unrelated artifacts, trust an evidence-owned target list, or upgrade a unit-test result into real CLI certification.

## 5. Promote the exact accepted candidate

Use **Promote accepted native CLI candidate** only after configuring the protected GitHub Environment:

`release-promotion`

Configure required reviewers for that environment.

Promotion inputs are explicit:

- candidate workflow run ID;
- acceptance workflow run ID;
- acceptance artifact name;
- exact candidate commit SHA;
- exact release tag.

The promotion job:

1. checks out the exact candidate commit;
2. downloads candidate artifacts from the specified candidate run;
3. downloads acceptance evidence from the specified acceptance run;
4. runs `verify-promotion.mjs`, which re-hashes every candidate file;
5. reruns the D28 acceptance gate;
6. verifies the tag matches the frozen package version;
7. generates `latest.json` from the downloaded candidate files;
8. creates the GitHub Release using those same files;
9. verifies the published updater manifest and asset URLs;
10. only then marks the release as Latest.

There is no build command in the promotion workflow.

## 6. Canary policy

**Native CLI pinned/stable canary** records Claude Code and Codex CLI binary identities for pinned and stable channels.

Canary identity records are explicitly:

`NOT_CERTIFIED`

They exist to detect version movement and trigger re-certification. They are not acceptance evidence. If a pinned version variable is absent, the workflow emits a `BLOCKED` record instead of claiming success.

Repository variables:

- `CLAUDE_CANARY_PINNED_VERSION`
- `CODEX_CANARY_PINNED_VERSION`

## 7. Legacy release command

Direct stable publishing through:

```bash
npm run release -- --bump ...
npm run release -- --exact ...
```

is disabled with:

`DIRECT_RELEASE_DISABLED_USE_PROMOTION_WORKFLOW`

The legacy script remains callable only for the optional `--oss-only` mirror path after a GitHub Release already exists.

## 8. Updater signing

Required repository secrets:

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Private keys, test-account credentials, prompts, raw environment values, or auth tokens must never be committed to release evidence or normal logs.

## 9. Current certification boundary

Implementing this workflow does not certify a release.

Real Claude Code / Codex CLI Layer-C evidence still requires an explicitly authorized isolated target environment. If that evidence is unavailable, D20 remains `BLOCKED`, and D28 must reject promotion for any target that requires it.
