# Axon mutation contract

Read this reference before changing Axon storage or a storage-related artifact, or when reconciling a mutation whose outcome is uncertain.

## Respect the active backend

Axon discovers the backend from prescribed canonical state paths, without a configuration file. File state is worktree-local; SQLite is shared across Git worktrees. Both paths present is an error; invalid or pending state stops discovery. One backend per repository is supported, without scanning other worktrees. Inspect the intended root and canonical paths before mutation; do not treat a legacy backup as authoritative.

With the file backend, ordinary mutations change the active worktree's `.axon/state.jsonl`, which is intended for Git tracking and may already be tracked; `.gitattributes` and `.axon/.gitignore` belong to the same worktree-local artifact set. The mutation does not authorize staging, committing, merging, or discarding those files. Preserve unrelated working-tree changes and report the storage artifacts changed by the operation. Reads in one worktree observe only its current snapshot: they do not prove that another worktree has no divergent state or claim.

With SQLite in Git, the authoritative `.axon/axon.db` is under the parent of the common Git directory and may be outside the current sandbox or worktree. When host permission is required, limit escalation to the authorized Axon command whose access requirement has been established. This includes read commands that need write access for locking or supported automatic storage updates. Permission for one command does not extend to other commands or unrelated programs; follow the host's permission process for each required operation.

If the file backend's Git index is unmerged, normal operations are intentionally rejected. Preserve the inputs and resolve and stage a validated snapshot through the storage or merge workflow; do not bypass the guard by writing the state file directly.

## Execute one effect at a time

- Run each state-changing Axon command as a standalone shell call so another command cannot hide its exit status.
- Keep read-only discovery and unrelated programs out of the same shell call.
- Use the exact target and payload authorized by the caller. Do not broaden a selector, add relationships, choose another Entity, or continue into a later phase implicitly.

## Verify observed state

After a successful mutation, read the complete target or artifact state and verify the operation's postconditions. Re-read Revisions, Notes, relationships, claims, frontiers, or storage artifacts when the capability's effect can change them. Treat command output as evidence, not as a substitute for the relevant postcondition.

If a multi-phase workflow completes only some mutations, stop at the first unresolved phase. Preserve the applied state and any recovery artifact, report completed and remaining phases separately, and do not automatically roll back with compensating mutations.

## Retry only after reconciliation

A clear failure is not permission to repeat the same command without changing its cause. If command completion or storage application is unknown, first confirm that the process has ended and inspect current state using stable IDs, record counts, actor labels, payloads, and operation-specific postconditions.

Repeat a mutation only when its capability defines a safe reconciliation rule and the observations establish that repetition cannot duplicate the effect. In particular, Entity creation and Note addition need their own duplicate checks. If the evidence cannot distinguish applied from unapplied, report the storage outcome as unknown and stop.

## Preserve input snapshots

For a mutation sourced from a file or stdin, preserve the exact bytes before the first attempt when later re-reading could change the payload. Record a digest when the operation's retry contract needs one, and reuse only those verified bytes for an allowed retry. Do not silently re-read a mutable source.

Remove a temporary snapshot only after the operation is verified as applied or not applied. Preserve its exact path and digest when it is needed to reconcile an unknown or partial result.
