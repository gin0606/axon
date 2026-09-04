---
name: capture
description: Create a new Issue or Group as Undecided without adopting it. Use for an unresolved concern or plan boundary; not for accepted registration, existing-Entity changes, or implementation.
---

# Capture an undecided Axon Entity

Use `axon-kit:conventions`. Read its model, creation, and mutation contracts before creating an Entity.

## Input contract

The caller supplies an unresolved concern or plan boundary, the intended kind, and any intended parent or outgoing dependencies. This capability may make the wording durable, but it does not adopt the work, start it, schedule it, choose whether another Entity should be reused, or invent structural relationships. Route a requested Resurface-condition change to `axon-kit:triage` after capture.

Use an Issue for one concern or prospective work item. Use a Group only for an explicit plan boundary that may contain multiple Entities. If the intended kind or relationship is unresolved and affects the declaration, return `input required` without writing.

## Build the draft declaration

Use the declaration content supplied by the caller without adding a decision or requirement. Do not use the description for future findings or handoffs.

Before mutation, reread the proposed declaration as if the later reader had no access to the current conversation. Resolve unexplained local shorthand and ambiguous references.

## Create and verify

Run `axon capture <title>` for an Issue or `axon group capture <title>` for a Group. Include an already-decided parent with `--parent <group-id>` and the initial description with `-m <description>` or `-F <snapshot>` so identity, containment, and initial text are created atomically. Apply the mutation contract's frozen-input rule before using a file or stdin.

While the Entity remains `Undecided`, set requested outgoing dependencies with their dedicated commands, one mutation at a time. Verify the complete draft after every partial phase that could leave useful state. Do not apply a relationship that the caller did not supply.

For a newly created Entity, read the complete final Entity context and verify:

- the intended kind, title, description, parent, and outgoing dependencies
- `Progress=NotStarted`, `Disposition=Undecided`, and `Resurface condition=Always`
- zero Declaration Revisions
- no unintended Note or Control-state change

If creation succeeds but a later declaration phase fails, return the one allocated ID and the remaining phases. Do not create a replacement Entity. Reconcile an uncertain create through the creation contract before any retry.

Return the created ID, kind, complete declaration summary, and `DB applied` classification. Do not continue into adoption or work.
