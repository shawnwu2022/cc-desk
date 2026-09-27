# D20 target-machine real CLI certification command

Status: **execution harness ready; real Claude Code / Codex Layer-C evidence is still NOT_RUN until a target machine supplies the actual binaries, real lane drivers and an explicitly authorized test account.**

## Entry point

Run from the repository root:

```text
node scripts/native-cli/run-real-cli-certification.mjs --config <config.json>
```

Optional per-cell timeout:

```text
--timeout-ms <1..900000>
```

Exit codes are stable:

- `0`: both Claude Code and Codex certified PASS;
- `1`: FAIL, including malformed command configuration or invalid/forged evidence;
- `2`: BLOCKED because required real-target evidence is unavailable.

The command writes one JSON result to stdout. Driver stdout is forbidden by the lower-level runner, so target-driver diagnostics must not be mixed into the certification result.

## Credential rule

Do not put test-account secrets directly in the JSON file.

A `testAccountEnv` value must be an exact environment-variable reference:

```json
{
  "testAccountEnv": {
    "CLAUDE_CODE_OAUTH_TOKEN": "${D20_CLAUDE_TEST_TOKEN}"
  }
}
```

At runtime the command resolves only the explicitly named variable. A literal value such as `"secret-token"` is rejected with `PLAINTEXT_TEST_ACCOUNT_ENV_FORBIDDEN`.

If `authorizedTestAccount` is not exactly `true`, the product is BLOCKED as `AUTHORIZED_TEST_ACCOUNT_UNAVAILABLE` **before credential references are parsed**.

If an explicitly referenced environment variable is absent or empty, that product is BLOCKED as `TEST_ACCOUNT_ENV_UNAVAILABLE`.

The wrapper separately projects only the non-secret host environment required to start local programs:

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

Other process environment values are not forwarded as host environment. Account variables are forwarded only through the explicit `testAccountEnv` mapping above.

## Example configuration

Use a **fresh absolute test root for every certification attempt**. The matrix intentionally refuses to overwrite existing fixture/evidence files.

Example Windows shape:

```json
{
  "schemaVersion": 1,
  "claude": {
    "cli": "claude",
    "authorizedTestAccount": true,
    "testRoot": "C:\\cc-desk-d20\\run-20260927-claude",
    "binaryPath": "C:\\cc-desk-d20\\run-20260927-claude\\bin\\claude.exe",
    "drivers": {
      "ccDesk": "C:\\cc-desk-d20\\run-20260927-claude\\drivers\\cc-desk-driver.exe",
      "systemTerminal": "C:\\cc-desk-d20\\run-20260927-claude\\drivers\\system-terminal-driver.exe"
    },
    "nonce": "d20-claude-20260927-unique",
    "originalText": "d20-claude-20260927-unique\n你好\n<pasted_content id=\"literal\">keep literal</pasted_content id=\"literal\">\n",
    "transformId": "claude-user-prompt-submit-v1-exact",
    "testAccountEnv": {
      "CLAUDE_CODE_OAUTH_TOKEN": "${D20_CLAUDE_TEST_TOKEN}"
    }
  },
  "codex": {
    "cli": "codex",
    "authorizedTestAccount": true,
    "testRoot": "C:\\cc-desk-d20\\run-20260927-codex",
    "binaryPath": "C:\\cc-desk-d20\\run-20260927-codex\\bin\\codex.exe",
    "drivers": {
      "ccDesk": "C:\\cc-desk-d20\\run-20260927-codex\\drivers\\cc-desk-driver.exe",
      "systemTerminal": "C:\\cc-desk-d20\\run-20260927-codex\\drivers\\system-terminal-driver.exe"
    },
    "nonce": "d20-codex-20260927-unique",
    "originalText": "d20-codex-20260927-unique\n你好\n<pasted_content id=\"literal\">keep literal</pasted_content id=\"literal\">\n",
    "transformId": "codex-user-prompt-submit-v1-exact",
    "testAccountEnv": {
      "OPENAI_API_KEY": "${D20_CODEX_TEST_TOKEN}"
    }
  }
}
```

The variable names above are examples of explicit test-account transport, not a requirement that a particular installed CLI version authenticate through those variables. If the selected real CLI uses browser/keychain authentication instead, omit `testAccountEnv` and let the real target driver perform the authorized login flow inside the isolated target environment.

PowerShell example:

```powershell
$env:D20_CLAUDE_TEST_TOKEN = "<dedicated-authorized-test-token>"
$env:D20_CODEX_TEST_TOKEN = "<dedicated-authorized-test-token>"
node scripts/native-cli/run-real-cli-certification.mjs --config C:\cc-desk-d20\d20.json
$LASTEXITCODE
```

Do not commit the generated config, evidence directory or credentials.

## Required target material

For each product, the following paths must be absolute and contained by that product's `testRoot`:

- `binaryPath`: the actual selected CLI executable/materialized binary identity;
- `drivers.ccDesk`: driver that exercises the real CC Desk native-terminal lane;
- `drivers.systemTerminal`: driver that exercises the real system-terminal comparison lane.

Execution re-checks containment with real paths, so a symlink that escapes the test root is BLOCKED.

Each driver receives:

```text
--cli
--binary
--fixture
--report
--lane
--observer
```

and must write exactly one evidence document to `--report`, emit no stdout, and exit zero only after the real target attempt has completed. The evidence must satisfy `scripts/native-cli/real-cli-evidence.mjs`.

A synthetic hook, screen scraper, model repetition, fake binary identity or writer-success receipt is not a valid replacement.

## Four-cell requirement

Each product runs exactly:

1. CC Desk / observer off;
2. CC Desk / observer on;
3. system terminal / observer off;
4. system terminal / observer on.

All four records must bind to the same target and fixture identity. PASS evidence is additionally checked against the SHA-256 of the actual `binaryPath` file that was executed.

The aggregate command returns PASS only when both products independently pass their four-cell comparison. One unavailable product keeps the aggregate D20 result BLOCKED.

## Current boundary

This command makes D20 target execution deterministic and repeatable. It deliberately does **not** provision a test account, copy an installed CLI, fabricate a system terminal, or synthesize the CC Desk lane. Those are target-environment facts and must be supplied by the real certification machine.
