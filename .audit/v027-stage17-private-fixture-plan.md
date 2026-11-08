# Architecture checkpoint: 010 private-source fixture proposal

Historical plan checkpoint; subsequent authorized implementation is recorded in
`v027-stage18-private-fixture.md`. The original failed evidence below is retained.

Status: **plan only, not implemented**. Current head remains
`8a9cb8c485614c6dedcf7cc9b0eea6a83a06c883`.

The third semantic attempt still failed on Windows-2022:
[D12 42462584248](https://github.com/shawnwu2022/cc-desk/actions/runs/42462584248),
job `128674862614`, **261 passed / 1 failed**. 010 again retained its three
explicit ACEs and acquired three inherited duplicates. Removing the explicit
UNPROTECTED request did not establish exact legacy restoration. The native call
succeeded; the unchanged postcondition correctly refused the result. 012's
success does not substitute for 010. The current visual workflow passed.

No fourth restoration-API experiment is implemented. The adopted systematic
debugging skill requires an architecture discussion after three failed fixes.
The problem now is the fixture's unstated dependency on a host's legacy ACL,
not a reason to accept additional ACEs or exclude a safety test.

Independent read-only review recommends A, narrowly scoped to 010:

1. Add an explicit private-source fixture construction mode used only by 010.
   Keep the default constructor for 009/012 and the other SDK experiments.
2. Before writing original transcripts or acquiring old DELETE handles, create
   the project source with the existing `PrivateDirectory::create_new` primitive
   (`version_history/windows/files.rs:750`). It creates with the current user's
   protected OI/CI descriptor, then verifies owner and private ACL through the
   held object. This is task-created synthetic data only.
3. Explicitly verify the private protected starting state and that current-user
   creation succeeds before the denial and fails after it. The creation handle
   must be released before TempDir cleanup, with no unintended sharing-mode
   change to the original experiment.
4. Retain every original classic/Ex migration-success, outside-original-present,
   source-name-absent, replacement-refused, identity-lease, guard-order, strict
   restoration and post-restoration-create assertion. Do not substitute a local
   observation for fresh hosted 010 success.
5. Document the claim accurately: the old DELETE capability escapes even this
   stricter private protected source. Arbitrary legacy ACL byte restoration
   remains unsupported/unproved; Claude production deletion remains disabled.

The exact native primitive was read: its creation access requests directory
list/traverse/read-attributes/READ_CONTROL/SYNCHRONIZE, rather than write-data or
DELETE; it verifies a current-user protected descriptor at creation. Lifecycle
and ordinary guard compatibility still require implementation tests.

Alternative B preserves a general legacy restoration contract. It first needs
a controlled local fixture matching the hosted `0x8004`, three-explicit-ACE
starting state, followed by a narrowly documented exact-restoration design.
Switching to a held handle alone is insufficient proof: SetSecurityInfo also
documents inheritance propagation. SetKernelObjectSecurity explicitly excludes
filesystem objects. No API is blindly substituted or permission widened.

This plan changes no code, tests, exclusions, release policy or real permissions.
It is intentionally an uncommitted review material so it does not trigger another
known-failing full CI run solely to record the checkpoint. The existing instance,
source leases, denied observations and all earlier failure evidence are retained.
