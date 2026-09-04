---
name: work-state
description: Entityの実作業に合わせてstart、release、doneを反映する。明示された進行操作または外部作業を所有するworkflowから使い、実装、Issue選択、採否判断、commitは行わない。
---

# Synchronize Axon work state

Use `axon-kit:conventions`. Read its model and mutation contracts before a lifecycle mutation. Use `axon-kit:add-note` separately when the calling workflow has a material result or handoff to record.

This capability changes only Progress and claim through `start`, `release`, or `done`. The caller owns any implementation, investigation, planning, review, and commit workflow outside Axon.

## Start

Read the complete Entity context and verify the target itself is `ready`. `axon start` checks readiness and acquires the claim in one transaction, but the calling workflow still needs the declaration and history before beginning external work.

Run `axon start <id>` as a standalone mutation. Verify `Progress=InProgress`, the observed claim owner and worktree, and any Group frontier opened by the start.

If the Entity is already `InProgress`, do not steal or release its claim. Return the current owner and Notes so the calling workflow can decide whether it is resuming compatible work. If the target is Undecided, Rejected, blocked, orphaned, outside active scope, ended, or claimed elsewhere, return the exact state without changing another axis or choosing another Entity.

A Group start opens only its own activation gate. It never starts descendants. Return the newly exposed `ready` and `triage` frontier without choosing a child.

## Release

Release means a temporary stop or handoff, not completion. The caller supplies the decision to release and its reason. If a material handoff is needed, require the calling workflow to append and verify it through `axon-kit:add-note` before releasing; a state-change reason does not replace the Note and must not be duplicated in it.

For a Group, verify that it has zero `InProgress` descendants. Do not release descendants as a side effect.

Run `axon release <id> -r <reason>` as a standalone mutation. Verify `Progress=NotStarted`, claim removal, progress history, and relevant `list`, `ready`, and `triage` changes. If release fails or its outcome is unknown after a Note was recorded, never append that Note again; return its number with the current state.

## Done

Done means no further work will be performed for this Entity. The caller must establish which of these outcomes applies:

- an Accepted Entity's declared purpose is satisfied and no work remains
- a Rejected Entity's in-progress work is permanently stopped
- an Undecided Entity's in-progress work is permanently stopped

Do not treat a pause, failed test, unresolved review finding, or uncommitted required result as completion merely to clear a claim.

For a Group, also require every descendant to be terminal. Do not mutate descendants or automatically finish the Group when the last child becomes terminal.

Before `done`, enumerate all Entity IDs with `axon list`, read each complete context, and find every `AfterEntity(<target>)` waiter that refers to the target. Preserve the waiter IDs and their Group ancestry for post-mutation verification.

If the external work produced a material result or validation record, require the calling workflow to append and verify a concise result through `axon-kit:add-note` before completion. Do not create a routine completion Note.

Run `axon done <id>` as a standalone mutation. Verify the target's ended state, claim removal, progress history, all Declaration Revisions and result Notes, direct dependents, discovered waiters, Group ancestry, and `list`, `ready`, and `triage` impact. Do not automatically finish an ancestor Group; return any ancestor that has become completable.

## Return

Return the requested lifecycle effect, final Progress and claim, relevant Note numbers, frontier and relationship impact, and `DB applied` classification. Never continue into external work or a later lifecycle effect on your own.
