---
name: capture
description: Create a new Issue or Group as Undecided without adopting it. Use for an unresolved concern or plan boundary; not for accepted registration, existing-Entity changes, or implementation.
---

# Capture an undecided Axon Entity

Use `axon-kit:conventions`. Read its model, creation, and mutation contracts before creating an Entity.

## Input contract

The caller supplies an unresolved concern or plan boundary, the intended kind, and any intended parent, outgoing dependencies, or initial Resurface condition. This capability may make the wording durable, but it does not adopt the work, start it, schedule it, choose whether another Entity should be reused, or invent structural relationships. It can save a supplied initial condition; route later Resurface-condition changes to `axon-kit:triage`.

Use an Issue for one concern or prospective work item. Use a Group only for an explicit plan boundary that may contain multiple Entities. If the intended kind or relationship is unresolved and affects the declaration, return `input required` without writing.

## Build the draft declaration

Use the declaration content supplied by the caller without adding a decision or requirement. Do not use the description for future findings or handoffs.

Before mutation, reread the proposed declaration as if the later reader had no access to the current conversation. Resolve unexplained local shorthand and ambiguous references.

## Create and verify

Run `axon capture <title>` for an Issue or `axon group capture <title>` for a Group. Include an already-decided parent with `--parent <group-id>`, repeat `--needs <entity-id>` for outgoing dependencies, and supply the initial description with `-m <description>` or `-F <snapshot>`. Apply the mutation contract's frozen-input rule before using a file or stdin.

For a supplied initial condition, choose exactly one of `--manual`, `--at <YYYY-MM-DD>`, `--after <entity-id>`, or `--command <shell-string>`; omission means `Always`. IDs accept full IDs or unique suffixes. The complete draft and initial condition are saved atomically, without a Revision or fabricated state transitions. Do not invent a schedule or relationship that the caller did not supply.

Initial Command strings are not executed during creation. Use `axon show <id> --skip-command-evaluation` to verify the saved result without executing them.

For a newly created Entity, read the complete final Entity context and verify:

- the intended kind, title, description, parent, and outgoing dependencies
- `Progress=NotStarted`, `Disposition=Undecided`, and the supplied Resurface condition (default `Always`)
- zero Declaration Revisions
- no claim, Note, or decision/progress transition history

A clear creation failure leaves no partial Entity. Reconcile an uncertain create through the creation contract before any retry; do not create a replacement merely because the Entity is absent from a frontier.

Return the created ID, kind, complete declaration summary, and `DB applied` classification. Do not continue into adoption or work.
