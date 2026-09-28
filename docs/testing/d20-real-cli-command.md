# D20 target-machine real CLI certification command

Status: **the execution harness is ready; real Claude Code / Codex CLI Layer-C evidence remains BLOCKED until an authorized target environment supplies the actual binaries, real lane drivers and a dedicated test account.**

## Entry point

Run from the repository root:

```text
node scripts/native-cli/run-real-cli-certification.mjs --config <config.json>
```

Optional per-cell timeout:

```text
--timeout-ms <1..900000>
```

Stable exit codes:

- `0`: both Claude Code and Codex independently PASS all required cells;
- `1`: FAIL, including malformed configuration, driver failure or invalid/forged evidence;
- `2`: BLOCKED because required real-target material is unavailable or unverifiable.

The command writes one aggregate JSON result to stdout. Driver stdout is forbidden, preventing native diagnostics or secrets from being mixed into the certification result.

## Credential boundary

Do not place account secrets directly in the JSON configuration.

A `testAccountEnv` value must be an exact environment-variable reference:

```json
{
  "testAccountEnv": {
    "CLAUDE_CODE_OAUTH_TOKEN": "${D20_CLAUDE_TEST_TOKEN}"
  }
}
```

Literal values are rejected with `PLAINTEXT_TEST_ACCOUNT_ENV_FORBIDDEN`.

If `authorizedTestAccount` is not exactly `true`, the product becomes `AUTHORIZED_TEST_ACCOUNT_UNAVAILABLE` before credential references, CLI paths or native configuration are inspected.

The wrapper forwards only the non-secret host environment needed to start local programs:

```text
PATH / Path
LANG
LC_ALL
TERM
SHELL
SystemRoot
WINDIR
ComSpec
PATHEXT
TEMP
TMP
TMPDIR
```

Account variables are forwarded only through the explicit `testAccountEnv` mapping.

## Example configuration

Use a fresh absolute root for every certification attempt. The runner refuses to overwrite fixture or evidence files.

```json
{
  "schemaVersion": 1,
  "claude": {
    "cli": "claude",
    "authorizedTestAccount": true,
    "testRoot": "C:\\cc-desk-d20\\run-claude-unique",
    "binaryPath": "C:\\cc-desk-d20\\run-claude-unique\\bin\\claude.exe",
    "drivers": {
      "ccDesk": "C:\\cc-desk-d20\\run-claude-unique\\drivers\\cc-desk-driver.exe",
      "systemTerminal": "C:\\cc-desk-d20\\run-claude-unique\\drivers\\system-terminal-driver.exe"
    },
    "nonce": "d20-claude-unique",
    "originalText": "d20-claude-unique\n你好\n<pasted_content id=\"literal\">keep literal</pasted_content id=\"literal\">\n",
    "transformId": "claude-user-prompt-submit-v1-exact",
    "testAccountEnv": {
      "CLAUDE_CODE_OAUTH_TOKEN": "${D20_CLAUDE_TEST_TOKEN}"
    }
  },
  "codex": {
    "cli": "codex",
    "authorizedTestAccount": true,
    "testRoot": "C:\\cc-desk-d20\\run-codex-unique",
    "binaryPath": "C:\\cc-desk-d20\\run-codex-unique\\bin\\codex.exe",
    "drivers": {
      "ccDesk": "C:\\cc-desk-d20\\run-codex-unique\\drivers\\cc-desk-driver.exe",
      "systemTerminal": "C:\\cc-desk-d20\\run-codex-unique\\drivers\\system-terminal-driver.exe"
    },
    "nonce": "d20-codex-unique",
    "originalText": "d20-codex-unique\n你好\n<pasted_content id=\"literal\">keep literal</pasted_content id=\"literal\">\n",
    "transformId": "codex-user-prompt-submit-v1-exact",
    "testAccountEnv": {
      "OPENAI_API_KEY": "${D20_CODEX_TEST_TOKEN}"
    }
  }
}
```

The account variable names are examples, not a requirement that a particular CLI authenticate through environment variables. A real lane driver may instead perform an explicitly authorized browser/keychain login inside the isolated target environment.

Do not commit the configuration, evidence directory or credentials.

## Four-cell requirement

Each product must run exactly:

1. CC Desk lane / observer off;
2. CC Desk lane / observer on;
3. system-terminal lane / observer off;
4. system-terminal lane / observer on.

The aggregate command returns PASS only when both products independently pass all four cells.

## Filesystem isolation

For each product, `binaryPath` and both driver paths must be absolute and contained by that product's `testRoot`.

Execution rechecks containment with real paths. It rejects:

- binary or driver symlink escape;
- run/config/HOME/project subdirectory escape;
- fixture creation outside the real test root;
- report files whose real path escapes the test root;
- reused/non-fresh fixture or report paths.

Every cell receives isolated HOME, config, project, fixture and report locations.

## Driver contract

Each driver receives:

```text
--cli
--binary
--fixture
--report
--lane
--observer
```

The driver must:

- exercise the real target lane;
- write exactly one evidence document to `--report`;
- emit no stdout;
- exit zero only after the target attempt has completed;
- retain raw hook evidence required by `real-cli-evidence.mjs`.

The runner computes the SHA-256 of the actual selected CLI binary and rejects a different recorded identity.

## Exact orchestrator binding

Internal evidence consistency is not sufficient.

For every PASS cell, the runner additionally binds the record to the exact plan it created:

- CLI kind;
- outer run ID;
- lane and observer state;
- fixture SHA-256;
- nonce;
- original text;
- host payload bytes;
- transform ID;
- per-cell project cwd by real-path identity;
- actual selected CLI binary SHA-256.

For the CC Desk lane, native input-frame evidence must bind the exact outer run ID and positive generation/input sequence/mode epoch. Observer off/on records for a lane must also report the same host identity.

A self-consistent substituted fixture, forged cwd, unrelated native frame, malformed record, screen scraper, model repetition or writer-success receipt cannot become PASS.

## Remaining trust boundary

The target machine supplies the actual lane drivers and authorized account. Those drivers are reviewed target-environment material, not something the unit harness can manufacture.

Synthetic drivers in automated tests verify fail-closed behavior only. They do not constitute Layer-C evidence.

If either product, binary, account, hook schema, driver or evidence binding is unavailable, D20 remains BLOCKED.
