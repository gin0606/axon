---
name: capture
description: 新しいIssueまたはGroupを、採否を決めずUndecidedとして記録する。未判断の懸念や計画範囲に使い、採用済みの登録、既存Entityの変更、実装には使わない。
---

# Capture an undecided Axon Entity

Use `axon-kit:conventions`. Read its model, creation, and mutation contracts before creating an Entity.

## Input contract

The caller supplies an unresolved concern or plan boundary and any intended parent or outgoing dependencies. This capability may make the wording durable and identify likely duplicates, but it does not adopt the work, start it, schedule it, or invent structural relationships. Route a requested Resurface-condition change to `axon-kit:triage` after capture.

Use an Issue for one concern or prospective work item. Use a Group only for an explicit plan boundary that may contain multiple Entities. If the intended kind or relationship is unresolved and affects the declaration, return `input required` without writing.

Inspect current Entities using the creation contract. If an exact unfinished duplicate is already `Undecided`, return it as reused with its actual Progress and declaration instead of creating another. If the duplicate is `Accepted` or `Rejected`, return the conflicting existing decision to the caller; do not change its Disposition or allocate a replacement. Return an ended or structurally ambiguous candidate unless the caller already supplied a decision that distinguishes this new Entity.

## Build the draft declaration

Write an Issue title as the observed problem or unresolved work, without prematurely fixing a solution. Its optional description should make the observation and expected outcome recoverable. Write a Group title and optional description as the plan boundary and why its members belong together.

Include reproduction context, attempted actions, or a current workaround only when they materially support later triage. Keep a proposed solution visibly separate from observations. Do not use description for future findings or handoffs.

Before mutation, reread the proposed declaration as if the later reader had no access to the current conversation. Resolve unexplained local shorthand and ambiguous references.

## Create and verify

Run `axon capture <title>` for an Issue or `axon group capture <title>` for a Group. Include an already-decided parent with `--parent <group-id>` and the initial description with `-m <description>` or `-F <snapshot>` so identity, containment, and initial text are created atomically. Apply the mutation contract's frozen-input rule before using a file or stdin.

While the Entity remains `Undecided`, set requested outgoing dependencies with their dedicated commands, one mutation at a time. Verify the complete draft after every partial phase that could leave useful state. Do not apply any relationship that was only suggested during duplicate analysis.

For a newly created Entity, read the complete final Entity context and verify:

- the intended kind, title, description, parent, and outgoing dependencies
- `Progress=NotStarted`, `Disposition=Undecided`, and `Resurface condition=Always`
- zero Declaration Revisions
- no unintended Note or Control-state change

If creation succeeds but a later declaration phase fails, return the one allocated ID and the remaining phases. Do not create a replacement Entity. Reconcile an uncertain create through the creation contract before any retry.

For a reused Undecided Entity, preserve and report its actual Progress, declaration, and history without mutating it. Return the created or reused ID, kind, complete declaration summary, and `DB applied` classification. Do not continue into adoption or work.
