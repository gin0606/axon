---
name: work-state
description: Synchronize an Entity's Progress and claim through start, release, or done. Use for an explicit lifecycle operation or from a workflow that owns the external work; it does not implement, select work, decide Disposition, or commit.
---

# Synchronize Axon work state

Use `axon-kit:conventions`. Read its model and mutation contracts before a lifecycle mutation. Notes are a separate information operation owned by the calling request or workflow.

This capability changes only Progress and claim through `start`, `release`, or `done`. The caller owns any implementation, investigation, planning, review, and commit workflow outside Axon.

## Start

Read the Entity context and verify the target itself is `ready`. `axon start` checks readiness and acquires the claim in one transaction.

Run `axon start <id>` as a standalone mutation. Verify `Progress=InProgress`, the observed claim owner and worktree, and any Group frontier opened by the start.

If the Entity is already `InProgress`, do not steal or release its claim. Return the current owner so the calling workflow can decide whether it is resuming compatible work. If the target is Undecided, Rejected, blocked, orphaned, outside active scope, ended, or claimed elsewhere, return the exact state without changing another axis or choosing another Entity.

A Group start opens only its own activation gate. It never starts descendants. Return the newly exposed `ready` and `triage` frontier without choosing a child.

## Release

Release changes an `InProgress` Entity back to `NotStarted` and removes its claim. The caller supplies the release request and optional reason. A release reason belongs in progress history; a supplemental handoff is a separate Note.

For a Group, verify that it has zero `InProgress` descendants. Do not release descendants as a side effect.

Run `axon release <id>` with `-r <reason>` when supplied, as a standalone mutation. Verify `Progress=NotStarted`, claim removal, progress history, and relevant frontier changes.

## Done

Done means no further work will be performed for this Entity. The caller supplies that lifecycle decision; this capability does not define the external workflow's completion criteria.

For a Group, require every descendant to be terminal. Do not mutate descendants or automatically finish the Group when the last child becomes terminal.

Before `done`, inspect dependencies, Group ancestry, and any waiter impact needed by the caller. Use `axon list --skip-command-evaluation` only when complete reverse `AfterEntity` impact is required, then evaluate relevant frontiers separately.

Run `axon done <id>` as a standalone mutation. Verify the target's ended state, claim removal, progress history, direct dependents, relevant waiters, Group ancestry, and frontier impact. Do not automatically finish an ancestor Group; return any ancestor that has become completable.

## Return

Return the requested lifecycle effect, final Progress and claim, relevant frontier and relationship impact, changed storage artifacts when relevant, and storage-result classification. Never continue into external work or a later lifecycle effect on your own.
