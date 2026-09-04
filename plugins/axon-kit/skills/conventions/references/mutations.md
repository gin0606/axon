# Axon mutation contract

Read this reference before changing the Axon DB or when reconciling a mutation whose outcome is uncertain.

## Execute one effect at a time

- Run each state-changing Axon command as a standalone shell call so another command cannot hide its exit status.
- Keep read-only discovery and unrelated programs out of the same shell call.
- Use the exact target and payload authorized by the caller. Do not broaden a selector, add relationships, choose another Entity, or continue into a later phase implicitly.
- If a linked worktree places the shared `.axon` outside the writable sandbox, retry only the rejected mutation command through the host approval mechanism. Do not widen permission for read commands or other programs.

## Verify observed state

After a successful mutation, read the complete target state and verify the operation's postconditions. Re-read Revisions, Notes, relationships, claims, or frontiers when the capability's effect can change them. Treat command output as evidence, not as a substitute for the relevant postcondition.

If a multi-phase workflow completes only some mutations, stop at the first unresolved phase. Preserve the applied state and any recovery artifact, report completed and remaining phases separately, and do not automatically roll back with compensating mutations.

## Retry only after reconciliation

A clear failure is not permission to repeat the same command without changing its cause. If command completion or DB application is unknown, first confirm that the process has ended and inspect current state using stable IDs, record counts, actor labels, payloads, and operation-specific postconditions.

Repeat a mutation only when its capability defines a safe reconciliation rule and the observations establish that repetition cannot duplicate the effect. In particular, Entity creation and Note addition need their own duplicate checks. If the evidence cannot distinguish applied from unapplied, report the DB outcome as unknown and stop.

## Preserve input snapshots

For a mutation sourced from a file or stdin, freeze the exact bytes before the first attempt when later re-reading could change the payload. Keep the snapshot private to the acting agent, record a digest when the operation's retry contract needs one, and reuse only those verified bytes for an allowed retry. Do not silently re-read a user-owned mutable source.

Remove an agent-owned temporary snapshot only after the operation is verified as applied or not applied. Preserve its exact path and digest when it is needed to reconcile an unknown or partial result.
