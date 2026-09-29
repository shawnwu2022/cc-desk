# Unified Workspace UX execution ledger

## Durable execution policy

This branch is an append-only remote checkpoint for the unified workspace implementation.

For every implementation task:

1. Write and run the task's failing tests.
2. Implement the smallest passing change.
3. Run the task's targeted verification locally.
4. Commit the complete task as an atomic commit.
5. Push the commit to `feat/unified-workspace-ux` immediately.
6. Verify that the remote branch HEAD equals the local commit SHA.
7. Record the task, test command, result, and remote SHA in this ledger.
8. Only then begin the next task.

Rules:

- Do not force-push or rewrite remotely checkpointed task commits.
- Do not open a pull request until the implementation and visual design are frozen.
- Feature-branch checkpoint pushes must not be used to trigger CI.
- If remote synchronization fails, create a Git bundle and source snapshot, retain the workspace, mark the task `BLOCKED_SYNC`, and do not count it as remotely complete.
- Final CI and the Windows installer workflow run only after Task 25 freeze.

## Current verified remote state

- Branch: `feat/unified-workspace-ux`
- Verified remote baseline before this ledger: `707e5a0ff100acb69550c1f600c7d97f87b8d01a`
- Design specification: complete
- Implementation plan: complete
- Task 1: complete and present in remote history
- Next implementation task: Task 2 — durable shared project and session catalog state

## Task checkpoints

| Task | Status | Tests | Remote commit |
|---|---|---|---|
| 1 | complete | relative time, session presentation, safe user-error mapping | `04370fdc22e41fa07ed61546d9b6e5d15ac69e35` |
| 2 | pending | — | — |
